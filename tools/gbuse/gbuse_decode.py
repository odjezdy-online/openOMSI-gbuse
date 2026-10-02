#!/usr/bin/env python3
"""
gbuse_decode.py - referenční dekodér databází z gBUSE1 (BUSE BS120 / vnitřní LED panely).

Ověřeno na ADledA.hex / ADledB.hex (gBUSE1 - 1.12). gBUSE0 (vnější panely) a gBUSE2b
mají jiný formát fontu; tabulky LIN/CIL/DRU/ZST se z nich přečtou, fonty zatím ne.

Použití:
    python3 gbuse_decode.py ADledA.hex -o out/          # JSON + PNG náhledy
    python3 gbuse_decode.py ADledA.hex --render 1001    # vykreslí zastávku 1001 do terminálu

Výstup:
    out/<jméno>.json        kompletní databáze (fonty jako sloupce, tabulky, CYK, DOP, charmap)
    out/font_eX.png         náhled každého fontu (vyžaduje Pillow)
    out/zst_preview.png     prvních 40 zastávek vykreslených na 112x8 (vyžaduje Pillow)

Formát (zjištěno reverzním inženýrstvím, viz PROMPT.md, oddíl "Formát gBUSE1"):
  * Intel HEX -> souvislý obraz od adresy 0, nevyplněné bajty = 0xFF.
  * Sekce začínají 16 B hlavičkou "XXX: gBUSE1 - 1.12" na 16B zarovnané adrese, data hned za
    ní na další 32B hranici; sekce končí bajtem 0xFF na místě dalšího záznamu.
  * FNT: fonty [id(E0..EF)][délka BE16][glyfy...], glyf = [kód][šířka][výška][šířka*ceil(výška/8) B],
    sloupcově, MSB = horní řádek. Za fonty tabulka kód->Windows-1250 (od kódu 0x20).
  * LIN: [id 3 ASCII][délka LE16][text], ZST: [id 4 ASCII][délka LE16][text].
  * CYK/DOP: [id 1 B][délka LE16][data].
  * Text: bajty 0x20-0x7F ASCII, 0x80-0xAF kódy dle charmapy (Kamenických), 0xB0-0xBF mezera
    mezi znaky = n-0xB0 sloupců, 0xC0-0xCF příznak (význam neověřen), 0xE0-0xEF výběr fontu,
    0xF0-0xFF piktogramy (glyfy ve fontu), 0x0D konec, 0x1B escape (ESC + 1 znak).
"""
import argparse, json, os, re, sys

# ---------------------------------------------------------------- Intel HEX
def load_image(path):
    if not path.lower().endswith(".hex"):
        return open(path, "rb").read()
    mem, upper = {}, 0
    for n, line in enumerate(open(path, encoding="ascii", errors="replace"), 1):
        line = line.strip()
        if not line.startswith(":"):
            continue
        b = bytes.fromhex(line[1:])
        if sum(b) & 0xFF:
            raise ValueError(f"{path}:{n}: špatný kontrolní součet Intel HEX")
        ln, addr, typ = b[0], (b[1] << 8) | b[2], b[3]
        if typ == 0:
            for i, x in enumerate(b[4:4 + ln]):
                mem[upper + addr + i] = x
        elif typ == 1:
            break
        elif typ == 2:
            upper = ((b[4] << 8) | b[5]) << 4
        elif typ == 4:
            upper = ((b[4] << 8) | b[5]) << 16
    img = bytearray(b"\xff" * (max(mem) + 1))
    for k, v in mem.items():
        img[k] = v
    return bytes(img)

# ---------------------------------------------------------------- sekce
SECTION_RE = re.compile(rb"([A-Z]{3}):.{0,2}gBUSE(\d\w*) ?- ?([\d.]+)")

def find_sections(img):
    out = []
    for m in SECTION_RE.finditer(img):
        start = m.start()
        data = (start + 0x20) & ~0x1F  # data na další 32B hranici
        out.append({"tag": m.group(1).decode(), "gbuse": m.group(2).decode(),
                    "version": m.group(3).decode(), "header": start, "data": data})
    return out

# ---------------------------------------------------------------- fonty (gBUSE1)
def parse_fonts(img, start):
    fonts, p = {}, start
    while p < len(img) and 0xE0 <= img[p] <= 0xEF:
        fid, ln = img[p], (img[p + 1] << 8) | img[p + 2]
        q, end, glyphs = p + 3, p + 3 + ln, {}
        while q < end:
            if q + 2 == end and img[q] == 0xFF and img[q + 1] == 0:
                q = end  # gBUSE1 1.00 (Košice): seznam glyfů končí dvojicí FF 00
                break
            # [kód][počet bajtů dat][výška][data]: sloupec má ceil(výška/8) bajtů, první bajt =
            # horní řádky, bity zarovnané nahoru (MSB = horní řádek). U fontů výšky <= 8 je
            # počet bajtů totéž co šířka; u gBUSE0 (výška 9, 10, 19...) je to šířka * 2 nebo * 3.
            code, nbytes, h = img[q], img[q + 1], img[q + 2]
            bpc = max((h + 7) // 8, 1)
            w = nbytes // bpc
            raw = img[q + 3:q + 3 + nbytes]
            cols = []
            for x in range(w):
                v = 0
                for k in range(bpc):
                    v = (v << 8) | raw[x * bpc + k]
                cols.append(v >> (bpc * 8 - h) if bpc * 8 > h else v)
            glyphs[code] = {"w": w, "h": h, "cols": cols}
            q += 3 + nbytes
        if q != end:
            raise ValueError(f"font {fid:02X}: nesedí délka ({q:#x} != {end:#x})")
        fonts[fid] = glyphs
        p = end
    return fonts, p

def parse_charmap(img, p):
    """Tabulka za fonty: index 0 = kód 0x20; hodnota = znak ve Windows-1250 (0 = nepoužito)."""
    while p < len(img) and img[p] == 0xFF:
        p += 1
    table = img[p:p + 0xC0]
    cmap = {}
    for i, v in enumerate(table):
        code = 0x20 + i
        if v:
            cmap[code] = bytes([v]).decode("cp1250", "replace")
    return cmap

# ---------------------------------------------------------------- tabulky
def table_idlen(img, start, default):
    """Délka id záznamu: 3 (LIN, CIL) nebo 4 (ZST, DRU; CIL v gBUSE0 1.21 s adresami).
    Správná délka je ta, se kterou řetěz záznamů dojde čistě k 0xFF."""
    for idlen in (default, 7 - default):
        p, ok, n = start, True, 0
        while p + idlen + 2 <= len(img) and img[p] != 0xFF:
            if not all(0x20 <= c < 0x7F for c in img[p:p + idlen]):
                ok = False; break
            ln = img[p + idlen] | (img[p + idlen + 1] << 8)
            end = p + idlen + 2 + ln
            if end > len(img) or (ln and img[end - 1] != 0x0D):
                ok = False; break
            p, n = end, n + 1
        if ok and n:
            return idlen
    return default

def parse_table(img, start, idlen):
    idlen = table_idlen(img, start, idlen)
    rows, p = [], start
    while p + idlen + 2 <= len(img) and img[p] != 0xFF:
        ident = img[p:p + idlen].decode("ascii", "replace")
        n = img[p + idlen] | (img[p + idlen + 1] << 8)
        rows.append((ident, img[p + idlen + 2:p + idlen + 2 + n]))
        p += idlen + 2 + n
    return rows

def parse_records(img, start):
    rows, p = [], start
    while p + 3 <= len(img) and img[p] != 0xFF:
        n = img[p + 1] | (img[p + 2] << 8)
        rows.append((img[p], img[p + 3:p + 3 + n]))
        p += 3 + n
    return rows

# ---------------------------------------------------------------- text
def tokens(raw):
    """Rozloží text na tokeny. Neznámé kódy zachová, ať je engine může později doplnit."""
    out, i = [], 0
    while i < len(raw):
        b = raw[i]
        if b == 0x0D:
            break
        if b == 0x1B and i + 1 < len(raw):
            out.append({"t": "esc", "v": chr(raw[i + 1])}); i += 2; continue
        if 0xE0 <= b <= 0xEF:
            out.append({"t": "font", "v": b})
        elif 0xB0 <= b <= 0xBF:
            out.append({"t": "spacing", "v": b - 0xB0})
        elif 0xC0 <= b <= 0xCF:
            out.append({"t": "flag", "v": b})
        elif b == 0x26:
            out.append({"t": "placeholder", "v": "&"})
        elif b == 0x0A:
            out.append({"t": "newline"})
        elif b == 0x0C:
            out.append({"t": "page"})
        else:
            out.append({"t": "glyph", "v": b})
        i += 1
    return out

def plain(raw, cmap):
    s = ""
    for tok in tokens(raw):
        if tok["t"] == "glyph":
            c = tok["v"]
            s += chr(c) if 0x20 <= c < 0x80 else cmap.get(c, f"[{c:02X}]")
        elif tok["t"] == "placeholder":
            s += "&"
        elif tok["t"] == "spacing" and tok["v"] >= 2 and s and not s.endswith(" "):
            s += " "  # jen pro čitelnost: velká mezera se v datech používá místo slova
    return s

def render(raw, fonts, default_font=0xE1, default_spacing=1, skip_placeholder=False):
    """Vrátí seznam sloupců (bitmask, bit 0 = spodní řádek fontu). Neznámý font -> výchozí."""
    font, sp, cols, first = default_font, default_spacing, [], True
    for tok in tokens(raw):
        if tok["t"] == "font":
            font = tok["v"] if tok["v"] in fonts else default_font
        elif tok["t"] == "spacing":
            sp = tok["v"]
        elif tok["t"] in ("glyph", "placeholder"):
            if tok["t"] == "placeholder" and skip_placeholder:
                continue
            g = fonts.get(font, fonts[default_font]).get(tok.get("v") if tok["t"] == "glyph" else 0x26)
            if g is None:
                continue
            if not first:
                cols.extend([0] * sp)
            cols.extend(g["cols"]); first = False
    return cols

def ascii_art(cols, h=8):
    return "\n".join("".join("#" if (c >> (h - 1 - y)) & 1 else "." for c in cols) for y in range(h))

# ---------------------------------------------------------------- CYK
def parse_cycle(data):
    """[00 00][položky...][FF][00 00][jména položek jako Pascal stringy].
    Položka je buď pole (5 B): [šířka ve sloupcích][DOP šablona][proměnná][režim][doba],
    nebo jediný bajt 00 = konec stránky. Pole se skládají vedle sebe zleva, dokud se vejdou
    do šířky panelu (linka 22 + cíl 112 = 134 ze 135; zóna 67 + čas 67); pole přes celou
    šířku (135) je stránka sama pro sebe. Šířky sedí s daty: nejširší LIN má 22 sloupců,
    nejširší ZST 103 (+ šipka z DOP 2). Režim (03 / 00) a doba (sekundy) jsou hypotéza."""
    p, pages = 2, []
    while p < len(data) and data[p] != 0xFF:
        if data[p] == 0:
            pages.append({"raw": "00", "break": True})
            p += 1
            continue
        rec = data[p:p + 5]
        pages.append({"raw": rec.hex(" "), "break": False, "width": rec[0], "dop": rec[1],
                      "var": rec[2], "mode": rec[3], "time": rec[4]})
        p += 5
    p += 1 + 2
    names = []
    while p < len(data):
        n = data[p]; p += 1
        names.append(data[p:p + n].decode("cp1250", "replace")); p += n
    return {"pages": pages, "names": names}

# ---------------------------------------------------------------- hlavička
def parse_header(img):
    """Hlavička obrazu. gBUSE1 (AA/AB): [typ][00][poslední sloupec = šířka - 1]...[výchozí font,
    mezera]...[ukazatele na sekce, big-endian; 00 3F = sekce chybí]. gBUSE0 (57/55): okna
    [y0 y1 x0 x1] pro číslo linky a pro cíl, výchozí font a mezera každého okna, ukazatele."""
    h = {"type": img[0]}
    if img[0] in (0xAA, 0xAB):
        h["kind"] = "gBUSE1 (vnitřní LED panel)"
        h["width"] = img[2] + 1
        h["rows"] = 8
        key = bytes([0xE3, 0x01, 0x0A])
        i = img.find(key, 0, 0x20)
        i = i if i >= 0 else None
        if i is not None:
            h["default_font"] = img[i]; h["default_spacing"] = img[i + 1]
            h["pointers"] = [(img[k] << 8) | img[k + 1] for k in range(i + 3, i + 3 + 16, 2)]
    elif img[0] in (0x57, 0x55):
        h["kind"] = "gBUSE0 (vnější panel)"
        h["windows"] = [{"y0": img[1], "y1": img[2], "x0": img[3], "x1": img[4]},
                        {"y0": img[5], "y1": img[6], "x0": img[7], "x1": img[8]}]
        h["rows"] = max(w["y1"] for w in h["windows"]) + 1
        h["width"] = max(w["x1"] for w in h["windows"]) + 1
    return h

# ---------------------------------------------------------------- main
def decode(path):
    img = load_image(path)
    secs = find_sections(img)
    db = {"file": os.path.basename(path), "size": len(img),
          "header_raw": img[:0x40].hex(" "), "sections": secs}
    m = re.search(rb"[A-Z]\w{5,7}_\d{6}", img[:0x40])
    db["name"] = m.group().decode() if m else None
    fonts, cmap = {}, {}
    db["header"] = parse_header(img)
    for s in secs:
        if s["tag"] == "FNT" and s["gbuse"] in ("0", "1"):
            fonts, end = parse_fonts(img, s["data"])
            cmap = parse_charmap(img, end)
    db["fonts"] = {f"{k:02X}": {f"{c:02X}": g for c, g in v.items()} for k, v in fonts.items()}
    db["charmap"] = {f"{k:02X}": v for k, v in cmap.items()}
    for s in secs:
        tag = s["tag"]
        if tag in ("LIN", "CIL"):
            rows = parse_table(img, s["data"], 3)
        elif tag in ("ZST", "DRU"):
            rows = parse_table(img, s["data"], 4)
        elif tag in ("CYK", "DOP"):
            rows = [(str(i), d) for i, d in parse_records(img, s["data"])]
        else:
            continue
        if tag == "CYK":
            db[tag] = [{"id": int(i), "raw": d.hex(" "), **parse_cycle(d)} for i, d in rows]
        else:
            db[tag] = [{"id": i, "raw": d.hex(" "), "text": plain(d, cmap), "tokens": tokens(d)}
                       for i, d in rows]
    return img, db, fonts

def save_png(path, strips, h=8, scale=4):
    from PIL import Image, ImageDraw
    width = max((len(c) for c in strips), default=1)
    im = Image.new("RGB", (width * scale, len(strips) * (h + 2) * scale), (20, 8, 6))
    dr = ImageDraw.Draw(im)
    for row, cols in enumerate(strips):
        for x, c in enumerate(cols):
            for y in range(h):
                on = (c >> (h - 1 - y)) & 1
                x0, y0 = x * scale, (row * (h + 2) + y) * scale
                dr.ellipse([x0, y0, x0 + scale - 1, y0 + scale - 1],
                           fill=(255, 74, 28) if on else (44, 16, 10))
    im.save(path)

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("file")
    ap.add_argument("-o", "--out")
    ap.add_argument("--golden", help="zapíše referenční vykreslení všech LIN/ZST/DOP textů do JSON (testy enginu)")
    ap.add_argument("--render", help="ID zastávky (ZST) nebo linky (LIN) k vykreslení do terminálu")
    a = ap.parse_args()
    img, db, fonts = decode(a.file)
    if a.golden:
        gold = {"note": "sloupce zleva, bit 7 = horní řádek; výchozí font E1, mezera 1, '&' vykreslen jako glyf 0x26 pokud existuje",
                "items": []}
        for tab in ("LIN", "ZST", "DOP"):
            for r in db.get(tab, []):
                gold["items"].append({"table": tab, "id": r["id"], "raw": r["raw"],
                                      "cols": render(bytes.fromhex(r["raw"]), fonts)})
        json.dump(gold, open(a.golden, "w"), indent=0)
        print(f"golden: {len(gold['items'])} položek -> {a.golden}")
        return
    if a.render:
        for tab in ("ZST", "LIN"):
            for r in db.get(tab, []):
                if r["id"] == a.render:
                    raw = bytes.fromhex(r["raw"])
                    print(f"{tab} {r['id']}: {r['text']}")
                    print(ascii_art(render(raw, fonts)))
        return
    if not a.out:
        json.dump({k: v for k, v in db.items() if k != "fonts"}, sys.stdout, ensure_ascii=False, indent=1)
        return
    os.makedirs(a.out, exist_ok=True)
    stem = os.path.splitext(os.path.basename(a.file))[0]
    with open(os.path.join(a.out, stem + ".json"), "w", encoding="utf-8") as f:
        json.dump(db, f, ensure_ascii=False, indent=1)
    try:
        for fid, glyphs in fonts.items():
            strips, line = [], []
            for c in sorted(glyphs):
                line += glyphs[c]["cols"] + [0, 0]
                if len(line) > 150:
                    strips.append(line); line = []
            strips.append(line)
            save_png(os.path.join(a.out, f"font_{fid:02X}.png"), strips)
        if "ZST" in db:
            strips = [render(bytes.fromhex(r["raw"]), fonts) for r in db["ZST"][1:41]]
            save_png(os.path.join(a.out, "zst_preview.png"), [s[:112] + [0] * (112 - len(s[:112])) for s in strips])
    except ImportError:
        print("Pillow není nainstalovaný, PNG náhledy přeskočeny (pip install pillow).", file=sys.stderr)
    print(f"{stem}: {len(db.get('ZST', []))} zastávek, {len(db.get('LIN', []))} linek, "
          f"{len(fonts)} fontů -> {a.out}")

if __name__ == "__main__":
    main()
