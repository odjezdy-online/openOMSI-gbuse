-- engine.lua - engine vnitřního LED panelu BUSE BS120 / BS190 (port crate buse-engine).
--
-- Čistá Lua 5.4 bez io: databázi dostane jako tabulku z buse_db.lua (generuje gbuse-convert),
-- čas zvenku jako dt. Musí dávat bit po bitu stejné snímky jako Rust engine; hlídají to
-- tests/run_golden.lua (render_text proti golden_render.json) a tests/run_sim.lua.

local E = {}

local byte, char, floor = string.byte, string.char, math.floor

---------------------------------------------------------------------------- text

-- Co se vykreslí na místě `&`: {kind="glyph"|"hide"|"blank"|"symbol"|"dop", ...}
local BOX = { 0x7F, 0x41, 0x41, 0x41, 0x7F }

local function default_render()
  return {
    default_font = 0xE1,
    default_spacing = 1,
    -- `&` žádný font brněnské databáze nemá, takže se nekreslí; `C0` je podle vnějších panelů
    -- svislý posun, ne příznak zastávky na znamení. Symbol jde zapnout v configu.
    placeholder = { kind = "hide" },
    placeholder_flag = nil,             -- nil = & se kreslí u všech záznamů
    placeholder_else = { kind = "hide" },
    missing_glyph = "fallback",         -- "fallback" | "box" | "skip"
    pict_alias = {},                    -- { {font=,glyph=,to_font=,to_glyph=}, ... }
  }
end

-- Chování referenčního dekodéru gbuse_decode.py (golden testy).
function E.reference_render()
  return {
    default_font = 0xE1,
    default_spacing = 1,
    placeholder = { kind = "glyph" },
    placeholder_flag = nil,
    placeholder_else = { kind = "glyph" },
    missing_glyph = "skip",
    pict_alias = {},
  }
end

local Log = {}
Log.__index = Log

function E.new_log()
  return setmetatable({ seen = {}, msgs = {} }, Log)
end

function Log:once(key, msg)
  if not self.seen[key] then
    self.seen[key] = true
    self.msgs[#self.msgs + 1] = msg
  end
end

function Log:take()
  local m = self.msgs
  self.msgs = {}
  return m
end

-- Má text daný příznak (C0-CF)? Čte jen do 0D a přeskakuje ESC sekvence.
local function has_flag(raw, flag)
  local i, n = 1, #raw
  while i <= n do
    local b = byte(raw, i)
    if b == 0x0D then return false end
    if b == 0x1B and i < n then
      i = i + 2
    else
      if b == flag then return true end
      i = i + 1
    end
  end
  return false
end
E.has_flag = has_flag

local function emit(out, st, cols, n)
  local k = #out
  if not st.first then
    for _ = 1, st.sp do
      k = k + 1
      out[k] = 0
    end
  end
  for i = 1, n do
    out[k + i] = cols[i]
  end
  st.first = false
end

local ZEROS = setmetatable({}, { __index = function() return 0 end })

local function glyph(out, st, log, font, code)
  local fonts, opts = st.db.fonts, st.opts
  local f = fonts[font] or fonts[opts.default_font]
  local g = f and f[code]
  if g then
    emit(out, st, g, #g)
    return
  end
  log:once(font * 256 + code, string.format("font %02X nemá glyf %02X", font, code))
  local df = fonts[opts.default_font]
  local alt = df and df[code]
  if opts.missing_glyph == "skip" then
    return
  elseif opts.missing_glyph == "fallback" and alt then
    emit(out, st, alt, #alt)
  else
    emit(out, st, BOX, 5)
  end
end

local function run(out, st, log, raw, ph, depth)
  local db, opts = st.db, st.opts
  local i, n = 1, #raw
  while i <= n do
    local b = byte(raw, i)
    if b == 0x0D then
      return
    elseif b == 0x1B and i < n then
      i = i + 2
    else
      i = i + 1
      if b >= 0xE0 and b <= 0xEF then
        if db.fonts[b] then
          st.font, st.unknown = b, nil
        else
          log:once(0x10000 + b, string.format("neznámý font %02X, použit výchozí", b))
          st.font, st.unknown = opts.default_font, b
        end
      elseif b >= 0xB0 and b <= 0xBF then
        st.sp = b - 0xB0
      elseif b >= 0xC0 and b <= 0xCF then
        -- příznak záznamu
      elseif b == 0x26 then
        local kind = ph.kind
        if kind == "glyph" then
          local f = db.fonts[st.font] or db.fonts[opts.default_font]
          local g = f and f[0x26]
          if g then emit(out, st, g, #g) end
        elseif kind == "blank" then
          emit(out, st, ZEROS, ph.n)
        elseif kind == "symbol" then
          glyph(out, st, log, ph.font, ph.glyph)
        elseif kind == "dop" then
          local dop = db.dop[ph.id]
          if dop and depth == 0 then
            local font, sp, unknown = st.font, st.sp, st.unknown
            run(out, st, log, dop, { kind = "hide" }, depth + 1)
            st.font, st.sp, st.unknown = font, sp, unknown
          end
        end
      elseif b == 0x0A or b == 0x0C then
        -- nový řádek / stránka (gBUSE0)
      else
        local done = false
        if st.unknown then
          for _, a in ipairs(opts.pict_alias) do
            if a.font == st.unknown and a.glyph == b then
              glyph(out, st, log, a.to_font, a.to_glyph)
              done = true
              break
            end
          end
        end
        if not done then glyph(out, st, log, st.font, b) end
      end
    end
  end
end

-- Vykreslí navazující části (DOP šablona + hodnota) do `out`; stav fontu a mezer přechází.
function E.render_parts(db, parts, opts, out, log)
  local flagged = true
  if opts.placeholder_flag then
    flagged = false
    for _, p in ipairs(parts) do
      if has_flag(p, opts.placeholder_flag) then flagged = true end
    end
  end
  local ph = flagged and opts.placeholder or opts.placeholder_else
  local st = { db = db, opts = opts, font = opts.default_font, unknown = nil, sp = opts.default_spacing, first = true }
  for _, p in ipairs(parts) do
    run(out, st, log, p, ph, 0)
  end
  return out
end

-- Vykreslí text na sloupce; bit 7 = horní řádek.
function E.render_text(db, raw, opts, log)
  return E.render_parts(db, { raw }, opts, {}, log or E.new_log())
end

function E.ascii_art(cols, h)
  h = h or 8
  local rows = {}
  for y = 0, h - 1 do
    local r = {}
    for x = 1, #cols do
      r[x] = ((cols[x] >> (h - 1 - y)) & 1) ~= 0 and "#" or "."
    end
    rows[#rows + 1] = table.concat(r)
  end
  return table.concat(rows, "\n")
end

---------------------------------------------------------------------------- názvy

local FOLD = {}
do
  local groups = {
    a = "áäâăą", A = "ÁÄÂĂĄ", c = "čćç", C = "ČĆÇ", d = "ďđ", D = "ĎĐ", e = "éěëę", E = "ÉĚËĘ",
    i = "íî", I = "ÍÎ", l = "ľĺł", L = "ĽĹŁ", n = "ňń", N = "ŇŃ", o = "óöôő", O = "ÓÖÔŐ",
    r = "řŕ", R = "ŘŔ", s = "šśşß", S = "ŠŚŞ", t = "ťţ", T = "ŤŢ", u = "úůüű", U = "ÚŮÜŰ",
    y = "ý", Y = "Ý", z = "žźż", Z = "ŽŹŻ",
  }
  for ascii, chars in pairs(groups) do
    for _, cp in utf8.codes(chars) do
      FOLD[cp] = byte(ascii)
    end
  end
end

local ABBREV = {
  nam = "namesti", n = "namesti", nadr = "nadrazi", zel = "zeleznicni", st = "stanice",
  zast = "zastavka", aut = "autobusove", autobus = "autobusove", hl = "hlavni", ul = "ulice",
  tr = "trida", sidl = "sidliste", nem = "nemocnice", rozc = "rozcesti", kr = "kralovo",
  sv = "svateho", gen = "generala", dr = "doktora", zs = "skola", ks = "konecna",
}

-- Bezpečná iterace po codepointech (neplatné UTF-8 bajty bere jako Latin-1).
local function codepoints(s)
  local out = {}
  if utf8.len(s) then
    for _, cp in utf8.codes(s) do out[#out + 1] = cp end
  else
    for i = 1, #s do out[i] = byte(s, i) end
  end
  return out
end
E.codepoints = codepoints

-- Normalizace: bez diakritiky, malá písmena, interpunkce -> mezery, rozvinuté zkratky.
function E.normalize(name)
  local flat = {}
  for i, cp in ipairs(codepoints(name)) do
    cp = FOLD[cp] or cp
    if cp >= 65 and cp <= 90 then
      flat[i] = char(cp + 32)
    elseif (cp >= 97 and cp <= 122) or (cp >= 48 and cp <= 57) then
      flat[i] = char(cp)
    else
      flat[i] = " "
    end
  end
  local words = {}
  for w in table.concat(flat):gmatch("%S+") do
    words[#words + 1] = ABBREV[w] or w
  end
  return table.concat(words, " ")
end

local function split_words(s)
  local w = {}
  for x in s:gmatch("[^ ]+") do w[#w + 1] = x end
  return w
end

-- Skóre shody dvou normalizovaných názvů: 100 = shodné, 0 = nic společného.
function E.match_score(a, b)
  if a == "" or b == "" then return 0 end
  if a == b then return 100 end
  local wa, wb = split_words(a), split_words(b)
  local n = math.max(#wa, #wb)
  local score = 0
  for i = 1, math.min(#wa, #wb) do
    local x, y = wa[i], wb[i]
    if x == y then
      score = score + 100
    elseif #x >= 3 and #y >= 3 and (x:sub(1, #y) == y or y:sub(1, #x) == x) then
      score = score + 70
    else
      return 0
    end
  end
  local v = math.min(score // n, 99) - 10 * math.min(math.abs(#wa - #wb), 3)
  return v > 0 and v or 0
end

local CP1250_HIGH = {}
do
  local s = "\u{A0}ˇ˘Ł¤Ą¦§¨©Ş«¬\u{AD}®Ż°±˛ł´µ¶·¸ąş»Ľ˝ľżŔÁÂĂÄĹĆÇČÉĘËĚÍÎĎĐŃŇÓÔŐÖ×ŘŮÚŰÜÝŢßŕáâăäĺćçčéęëěíîďđńňóôőö÷řůúűüýţ˙"
  local i = 0xA0
  for _, cp in utf8.codes(s) do
    CP1250_HIGH[i] = cp
    i = i + 1
  end
end

local SUSPECT = {}
for _, cp in utf8.codes("èìøùïòÈÌØÙÏÒ¾»¹ðþæêåãõûàÀ") do SUSPECT[cp] = true end

-- Opraví text, který vznikl čtením Windows-1250 jako Windows-1252.
function E.fix_mojibake(s)
  local cps = codepoints(s)
  local bad = false
  for _, cp in ipairs(cps) do
    if SUSPECT[cp] then bad = true break end
  end
  if not bad then return s end
  for i, cp in ipairs(cps) do
    if cp >= 0xA0 and cp <= 0xFF then
      cps[i] = CP1250_HIGH[cp]
    elseif cp == 0x9D then
      cps[i] = 0x165
    elseif cp == 0x8D then
      cps[i] = 0x164
    end
  end
  return utf8.char(table.unpack(cps))
end

local function trim(s)
  return (s:gsub("^%s+", ""):gsub("%s+$", ""))
end
E.trim = trim

-- Inverze charmapy: codepoint -> kód glyfu (první výskyt vyhrává, jako v Rustu).
local function char_index(db)
  if db._c2k then return db._c2k end
  local idx = {}
  for code = 0x80, 0xDF do
    local cp = db.charmap[code]
    if cp and not idx[cp] then idx[cp] = code end
  end
  db._c2k = idx
  return idx
end

local function char_to_code(db, cp)
  if cp >= 0x20 and cp <= 0x7E then return cp end
  return char_index(db)[cp]
end

local function usable(code)
  return code and code >= 0x20 and code < 0xB0 and code ~= 0x26
end

-- Převede text na kódy glyfů databáze (bez řídicích kódů) a připojí je do tabulky bajtů.
function E.encode_text(db, s, out)
  for _, cp in ipairs(codepoints(s)) do
    if cp == 0x26 then cp = 0x2B end
    local code = char_to_code(db, cp)
    if not usable(code) then
      code = char_to_code(db, FOLD[cp] or cp)
      if not usable(code) then code = nil end
    end
    if code then
      out[#out + 1] = code
    elseif not (cp < 0x20 or (cp >= 0x7F and cp < 0xA0)) then
      out[#out + 1] = 0x3F
    end
  end
  return out
end

-- Text záznamu pro párování názvů: bez `&`, piktogramů a glyfů v neznámých fontech.
function E.match_text(db, raw)
  local out, known = {}, true
  local i, n = 1, #raw
  while i <= n do
    local b = byte(raw, i)
    if b == 0x0D then break end
    if b == 0x1B and i < n then
      i = i + 2
    else
      i = i + 1
      if b >= 0xE0 and b <= 0xEF then
        known = db.fonts[b] ~= nil
      elseif b >= 0xB0 and b <= 0xBF then
        if b - 0xB0 >= 2 then out[#out + 1] = " " end
      elseif b >= 0xC0 and b <= 0xCF or b == 0x26 or b == 0x0A or b == 0x0C then
        -- nic
      elseif known and b < 0xAF then
        local cp = (b >= 0x20 and b < 0x80) and b or db.charmap[b]
        if cp then out[#out + 1] = utf8.char(cp) end
      end
    end
  end
  return table.concat(out)
end

-- Předpočítané normalizované názvy ZST.
function E.name_index(db)
  local names = {}
  for _, id in ipairs(db.zst_order) do
    local n = E.normalize(E.match_text(db, db.zst[id]))
    if n ~= "" then names[#names + 1] = { n, id } end
  end
  return names
end

-- Nejlepší shoda: {id=, score=}; při rovnosti skóre vyhrává první záznam v databázi.
function E.find_name(index, name)
  local n = E.normalize(name)
  local best_id, best = nil, 0
  for _, e in ipairs(index) do
    local score = E.match_score(n, e[1])
    if score > best then
      best_id, best = e[2], score
      if score == 100 then break end
    end
  end
  if best_id then return { id = best_id, score = best } end
end

---------------------------------------------------------------------------- konfigurace

local DEFAULT_DOP = { line = 5, stop = 1, dest = 2, time = 3, zone = 4, info = 7, linedest = 0 }

-- Stránka s jedním polem přes celý panel (width 0 = zbytek panelu).
local function single(dop, var, time_ms, slide, center)
  return { fields = { { dop = dop, var = var, width = 0, center = center } }, time_ms = time_ms, slide = slide }
end

local function page(var, secs)
  return single(DEFAULT_DOP[var] or 0, var, secs * 1000, true, true)
end

function E.default_config()
  local render = default_render()
  -- HYPOTÉZA 2.4: E5-E9 jsou piktogramové fonty ve firmwaru; glyf F8 v nich = piktogram.
  render.pict_alias = {
    { font = 0xE5, glyph = 0xF8, to_font = 0xE1, to_glyph = 0xF8 },
    { font = 0xE7, glyph = 0xF8, to_font = 0xE1, to_glyph = 0xF9 },
    { font = 0xE8, glyph = 0xF8, to_font = 0xE1, to_glyph = 0xF6 },
    { font = 0xE9, glyph = 0xF8, to_font = 0xE1, to_glyph = 0xF7 },
  }
  return {
    width = 0, rows = 1, align = "auto",   -- width 0 = auto: z hlavičky databáze (db.width)
    scroll_step_ms = 50, scroll_gap = 16, scroll_start_delay_ms = 0, scroll_full_pass = true,
    slide = "push", slide_steps = 8, slide_step_ms = 40,
    render = render,
    placeholder_dest = { kind = "hide" },
    use_cyk = true, cycle_idle = { 0, 2 }, cycle_stop = 6, cyk_time_unit_ms = 1000,
    var_map = { [0x01] = "line", [0x08] = "stop", [0x09] = "dest", [0x0A] = "info", [0x0D] = "time", [0x0E] = "zone" },
    cyk_effect_slides = true, hold_ms = 4000,
    -- (bez cyklů v databázi - verze 1.xx - má panel pevné stránky linka+cíl, pásmo+čas)
    fallback_pages = {
      { fields = { { dop = 5, var = "line", width = 22, center = true }, { dop = 2, var = "dest", width = 0, center = true } }, time_ms = 4000, slide = true },
      { fields = { { dop = 4, var = "zone", width = 67, center = true }, { dop = 3, var = "time", width = 0, center = true } }, time_ms = 4000, slide = true },
    },
    fallback_stop_pages = { page("stop", 6) },
    row2_pages = { page("stop", 4) },
    linedest_gap = 4, stop_on_change = true, stop_on_press = true,
    fallback_line_font = 0xE3, fallback_stop_font = 0xE1,
    extra = {},   -- klíče, které engine nezná (napojení na vůz)
  }
end

local function parse_bool(v)
  v = v:lower()
  if v == "1" or v == "true" or v == "yes" or v == "on" or v == "ano" then return true end
  if v == "0" or v == "false" or v == "no" or v == "off" or v == "ne" then return false end
  error("čekám true/false, ne '" .. v .. "'")
end

local function parse_num(v)
  return tonumber(trim(v)) or error("čekám číslo, ne '" .. v .. "'")
end

local function parse_hex(v)
  local s = trim(v):gsub("^0x", "")
  local n = s:match("^%x%x?$") and tonumber(s, 16)
  return n or error("čekám hex bajt, ne '" .. v .. "'")
end

local VAR_NAMES = {
  line = "line", linka = "line", stop = "stop", zastavka = "stop", nextstop = "stop",
  dest = "dest", cil = "dest", info = "info", time = "time", cas = "time",
  zone = "zone", zona = "zone", linedest = "linedest",
}

local function parse_var(v)
  return VAR_NAMES[trim(v):lower()] or error("neznámá proměnná stránky '" .. v .. "'")
end

local function split(s, sep)
  local out = {}
  for item in (s .. sep):gmatch("(.-)" .. sep) do out[#out + 1] = item end
  return out
end

local function parse_placeholder(v)
  local p = split(trim(v):lower(), ":")
  local kind = p[1]
  if kind == "hide" then return { kind = "hide" } end
  if kind == "glyph" then return { kind = "glyph" } end
  if kind == "blank" then return { kind = "blank", n = parse_num(p[2] or "5") } end
  if kind == "symbol" then return { kind = "symbol", font = parse_hex(p[2] or "E1"), glyph = parse_hex(p[3] or "F1") } end
  if kind == "dop" then return { kind = "dop", id = parse_num(p[2] or "1") } end
  error("neznámý placeholder '" .. v .. "'")
end

-- `line+dest:4, time:4`: `a+b` jsou pole vedle sebe (linka 22 sloupců, poslední zbytek, ostatní 67)
local function parse_group(item)
  local secs = 4
  local names, s = item:match("^(.-):(.*)$")
  if names then secs = tonumber(trim(s)) or error("čekám číslo, ne '" .. s .. "'") else names = item end
  local vars = {}
  for name in (names .. "+"):gmatch("(.-)%+") do vars[#vars + 1] = parse_var(name) end
  local pg = { fields = {}, time_ms = math.floor(secs * 1000), slide = true }
  for i, var in ipairs(vars) do
    if i > 4 then error("stránka '" .. item .. "': nejvýš 4 pole") end
    local width = (i == #vars) and 0 or (var == "line" and 22 or 67)
    pg.fields[i] = { dop = DEFAULT_DOP[var] or 0, var = var, width = width, center = true }
  end
  return pg
end

local function parse_pages(v)
  local out = {}
  for _, item in ipairs(split(v, ",")) do
    item = trim(item)
    if item ~= "" and item:find("+", 1, true) then
      out[#out + 1] = parse_group(item)
    elseif item ~= "" then
      local dop
      local a, d = item:match("^(.-)@(.*)$")
      if a then item, dop = a, parse_num(d) end
      local secs = 4
      local name, s = item:match("^(.-):(.*)$")
      if name then item, secs = name, parse_num(s) end
      local var = parse_var(item)
      out[#out + 1] = single(dop or DEFAULT_DOP[var] or 0, var, floor(secs * 1000), true, true)
    end
  end
  return out
end

local function parse_aliases(v)
  local t = trim(v):lower()
  if t == "none" or t == "off" or t == "" then return {} end
  local out = {}
  for _, item in ipairs(split(v, ",")) do
    local af, ag, bf, bg = item:match("^%s*(%x+):(%x+)%s*=%s*(%x+):(%x+)%s*$")
    if not af then error("alias '" .. item .. "': čekám FONT:GLYF=FONT:GLYF") end
    out[#out + 1] = { font = parse_hex(af), glyph = parse_hex(ag), to_font = parse_hex(bf), to_glyph = parse_hex(bg) }
  end
  return out
end

local function clamp(v, lo, hi)
  return math.max(lo, math.min(hi, v))
end

local SETTERS = {
  width = function(c, v)
    local l = v:lower()
    if l == "auto" or l == "0" then c.width = 0 else c.width = clamp(floor(parse_num(v)), 8, 1024) end
  end,
  rows = function(c, v) c.rows = clamp(floor(parse_num(v)), 1, 2) end,
  align = function(c, v)
    v = v:lower()
    if v ~= "auto" and v ~= "left" and v ~= "center" then error("align: auto | left | center") end
    c.align = v
  end,
  scroll_step_ms = function(c, v) c.scroll_step_ms = math.max(parse_num(v), 1) end,
  scroll_gap = function(c, v) c.scroll_gap = floor(parse_num(v)) end,
  scroll_start_delay_ms = function(c, v) c.scroll_start_delay_ms = parse_num(v) end,
  scroll_full_pass = function(c, v) c.scroll_full_pass = parse_bool(v) end,
  slide = function(c, v)
    v = v:lower()
    if v == "none" or v == "off" then c.slide = "none" elseif v == "push" then c.slide = "push" else error("slide: push | none") end
  end,
  slide_steps = function(c, v) c.slide_steps = clamp(floor(parse_num(v)), 1, 8) end,
  slide_step_ms = function(c, v) c.slide_step_ms = math.max(parse_num(v), 1) end,
  default_font = function(c, v) c.render.default_font = parse_hex(v) end,
  placeholder = function(c, v) c.render.placeholder = parse_placeholder(v) end,
  placeholder_else = function(c, v) c.render.placeholder_else = parse_placeholder(v) end,
  placeholder_dest = function(c, v) c.placeholder_dest = parse_placeholder(v) end,
  placeholder_flag = function(c, v)
    local l = v:lower()
    if l == "none" or l == "off" then c.render.placeholder_flag = nil else c.render.placeholder_flag = parse_hex(v) end
  end,
  missing_glyph = function(c, v)
    v = v:lower()
    if v ~= "skip" and v ~= "box" and v ~= "fallback" then error("missing_glyph: fallback | box | skip") end
    c.render.missing_glyph = v
  end,
  pict_alias = function(c, v) c.render.pict_alias = parse_aliases(v) end,
  use_cyk = function(c, v) c.use_cyk = parse_bool(v) end,
  cycle_idle = function(c, v)
    local out = {}
    for _, s in ipairs(split(v, ",")) do out[#out + 1] = floor(parse_num(s)) end
    c.cycle_idle = out
  end,
  cycle_stop = function(c, v) c.cycle_stop = floor(parse_num(v)) end,
  cyk_time_unit_ms = function(c, v) c.cyk_time_unit_ms = floor(parse_num(v)) end,
  cyk_effect_slides = function(c, v) c.cyk_effect_slides = parse_bool(v) end,
  var_map = function(c, v)
    local out = {}
    for _, item in ipairs(split(v, ",")) do
      local code, var = item:match("^(.-)=(.*)$")
      if not code then error("var_map '" .. item .. "': čekám KÓD=proměnná") end
      out[parse_hex(code)] = parse_var(var)
    end
    c.var_map = out
  end,
  hold_ms = function(c, v) c.hold_ms = floor(parse_num(v)) end,
  fallback_pages = function(c, v) c.fallback_pages = parse_pages(v) end,
  fallback_stop_pages = function(c, v) c.fallback_stop_pages = parse_pages(v) end,
  row2_pages = function(c, v) c.row2_pages = parse_pages(v) end,
  bs190_bottom = function(c, v) c.row2_pages = parse_pages(v) end,
  linedest_gap = function(c, v) c.linedest_gap = floor(parse_num(v)) end,
  stop_on_change = function(c, v) c.stop_on_change = parse_bool(v) end,
  stop_on_press = function(c, v) c.stop_on_press = parse_bool(v) end,
  fallback_line_font = function(c, v) c.fallback_line_font = parse_hex(v) end,
  fallback_stop_font = function(c, v) c.fallback_stop_font = parse_hex(v) end,
}

-- Nastaví jednu hodnotu; vrací true, když klíč engine zná. Chybná hodnota = error.
function E.config_set(cfg, key, value)
  local f = SETTERS[key]
  if not f then return false end
  f(cfg, value)
  return true
end

-- Načte text `klíč = hodnota` (stejný formát jako buse_panel.cfg). Vrací config a hlášky.
function E.parse_config(text)
  local cfg, warn = E.default_config(), {}
  for line in (text .. "\n"):gmatch("(.-)\r?\n") do
    line = line:gsub("^\u{FEFF}", ""):gsub("[#;].*$", "")
    local k, v = line:match("^%s*(.-)%s*=%s*(.-)%s*$")
    if k and k ~= "" then
      k = k:lower()
      local ok, res = pcall(E.config_set, cfg, k, v)
      if not ok then
        warn[#warn + 1] = k .. ": " .. tostring(res):gsub("^.-:%d+: ", "")
      elseif not res then
        cfg.extra[k] = v
      end
    end
  end
  return cfg, warn
end

---------------------------------------------------------------------------- panel

local CH_LINE, CH_DEST, CH_STOP, CH_INFO, CH_TIME, CH_ZONE, CH_PRESS = 1, 2, 4, 8, 16, 32, 64
local VAR_MASK = { line = CH_LINE, dest = CH_DEST, stop = CH_STOP, info = CH_INFO, time = CH_TIME, zone = CH_ZONE, linedest = CH_LINE | CH_DEST }

local MAX_FIELDS = 4

local BLANK = { fields = {}, time_ms = 0, slide = false }

local function spec_mask(spec)
  local m = 0
  for _, f in ipairs(spec.fields) do m = m | (VAR_MASK[f.var] or 0) end
  return m
end

local function spec_has(spec, var)
  for _, f in ipairs(spec.fields) do
    if f.var == var then return true end
  end
  return false
end

local Panel = {}
Panel.__index = Panel

-- Stránky cyklů `ids`: pole se skládají vedle sebe, dokud se vejdou do šířky panelu; pole přes
-- celý panel je stránka samo, položka „konec stránky" (brk) stránku uzavře.
local function cycle_pages(db, cfg, ids, log)
  local out, width = {}, cfg.width
  for _, id in ipairs(ids) do
    local c = db.cyk[id]
    if not c then
      log:once("cyk" .. id, "cyklus " .. id .. " v databázi není")
    else
      local pg, used = { fields = {}, time_ms = 0, slide = false }, 0
      local function flush()
        if #pg.fields > 0 then out[#out + 1] = pg end
        pg, used = { fields = {}, time_ms = 0, slide = false }, 0
      end
      for _, p in ipairs(c.pages) do
        if p.brk then
          flush()
        else
          local full = p.width >= width
          local fw = full and width or p.width
          if #pg.fields > 0 and (used + fw > width or #pg.fields == MAX_FIELDS) then flush() end
          pg.fields[#pg.fields + 1] = {
            dop = p.dop,
            var = cfg.var_map[p.var] or ("unknown:" .. p.var),
            width = full and 0 or p.width,
            center = true,   -- všechno na střed svého pole, i zóna a čas (tak to kreslí gBUSE1)
          }
          pg.time_ms = math.max(pg.time_ms, p.time * cfg.cyk_time_unit_ms)
          pg.slide = pg.slide or (not cfg.cyk_effect_slides) or p.mode ~= 0
          used = used + fw
        end
      end
      flush()
    end
  end
  return out
end

local function zeros(n)
  local t = {}
  for i = 1, n do t[i] = 0 end
  return t
end

local function new_row(width, idle, stop)
  return {
    idle = idle, stop = stop, mode = "idle", idx = 1, valid = false, fresh = false, started = false,
    spec = BLANK, cur = {}, old = zeros(width), vis = zeros(width),
    slide_left = 0, slide_acc = 0, scroll_off = 0, scroll_acc = 0, scroll_wait = 0, passes = 0, elapsed = 0,
  }
end

local function with_placeholder(render, ph)
  local o = {}
  for k, v in pairs(render) do o[k] = v end
  o.placeholder = ph
  o.placeholder_flag = nil
  return o
end

-- db = tabulka z buse_db.lua, cfg = E.default_config() / E.parse_config()
function E.new_panel(db, cfg)
  -- šířka `auto`: z hlavičky databáze (bez ní 135 = BS 120.0A)
  if cfg.width == 0 then cfg.width = clamp(db.width or 135, 8, 1024) end
  local log = E.new_log()
  local idle, stop = {}, {}
  if cfg.use_cyk then
    idle = cycle_pages(db, cfg, cfg.cycle_idle, log)
    stop = cycle_pages(db, cfg, { cfg.cycle_stop }, log)
    if #idle == 0 then log:once("nocyk", "CYK: žádné stránky klidového cyklu, použit záložní cyklus") end
  end
  if #idle == 0 then idle = cfg.fallback_pages end
  if #stop == 0 then stop = cfg.fallback_stop_pages end
  local rows
  if cfg.rows >= 2 then
    -- dvouřádek: nahoře klidový cyklus (linka + cíl, zóna + čas), dole zastávka
    rows = { new_row(cfg.width, idle, {}), new_row(cfg.width, cfg.row2_pages, stop) }
  else
    rows = { new_row(cfg.width, idle, stop) }
  end
  local strips = {}
  for i = 1, #rows do strips[i] = zeros(cfg.width) end
  return setmetatable({
    db = db, cfg = cfg, log = log, rows = rows,
    index = E.name_index(db),
    opts_dest = with_placeholder(cfg.render, cfg.placeholder_dest),
    opts_on = with_placeholder(cfg.render, cfg.render.placeholder),
    opts_off = with_placeholder(cfg.render, cfg.render.placeholder_else),
    seen = { line = "", dest_id = "", dest_name = "", stop_id = "", stop_name = "", zone = "", info = "", pressed = false, first = false },
    frame = { width = cfg.width, strips = strips },
    frame_no = 0, emitted = false,
  }, Panel)
end

local function pad_id(id, len)
  id = trim(id)
  if id ~= "" and #id <= len and id:match("^%d+$") then
    return string.rep("0", len - #id) .. id
  end
  return id
end

local function is_empty_ref(r)
  return r.id == "" and r.name == ""
end

local function bytes_to_string(t)
  local parts = {}
  for i = 1, #t, 64 do
    parts[#parts + 1] = char(table.unpack(t, i, math.min(i + 63, #t)))
  end
  return table.concat(parts)
end

function Panel:_dop(id)
  if id == 0 then return "" end
  return self.db.dop[id] or ""
end

function Panel:_line(dop, inp, out)
  if trim(inp.line) == "" then return false end
  local id = pad_id(inp.line, 3)
  local rec = self.db.lin[id]
  if rec then
    E.render_parts(self.db, { self:_dop(dop), rec }, self.cfg.render, out, self.log)
  else
    local raw = E.encode_text(self.db, trim(inp.line), { self.cfg.fallback_line_font, 0xB1 })
    E.render_parts(self.db, { self:_dop(dop), bytes_to_string(raw) }, self.cfg.render, out, self.log)
  end
  return true
end

-- dest: cíl; databáze verze 1.xx (Košice) mají cíle ve vlastní tabulce CIL s třímístným id
function Panel:_stop(dop, s, dest, opts, out)
  if is_empty_ref(s) then return false end
  local rec
  if dest and self.db.cil and trim(s.id) ~= "" then rec = self.db.cil[pad_id(s.id, 3)] end
  if not rec and trim(s.id) ~= "" then rec = self.db.zst[pad_id(s.id, 4)] end
  if not rec then
    local m = E.find_name(self.index, s.name)
    if m and m.score == 100 then rec = self.db.zst[m.id] end
  end
  if rec then
    E.render_parts(self.db, { self:_dop(dop), rec }, opts, out, self.log)
  else
    if trim(s.name) == "" then return false end
    local raw = E.encode_text(self.db, trim(s.name), { self.cfg.fallback_stop_font, 0xB1 })
    E.render_parts(self.db, { self:_dop(dop), bytes_to_string(raw) }, opts, out, self.log)
  end
  return true
end

function Panel:_text(dop, text, out)
  if trim(text) == "" then return false end
  local raw = E.encode_text(self.db, trim(text), {})
  E.render_parts(self.db, { self:_dop(dop), bytes_to_string(raw) }, self.cfg.render, out, self.log)
  return true
end

-- Vykreslí obsah stránky; nil = stránka je prázdná a přeskočí se.
-- Jedno pole přes celý panel: kratší text se zarovná a doplní na šířku panelu, delší zůstane
-- celý a běží. Víc polí vedle sebe: každé do svého okna, co se nevejde, je oříznuté.
function Panel:_build(spec, inp)
  local cfg = self.cfg
  local w = cfg.width
  local function center(f)
    if cfg.align == "auto" then return f.center end
    return cfg.align == "center"
  end
  local fields = spec.fields
  if #fields == 1 and (fields[1].width == 0 or fields[1].width >= w) then
    local f = fields[1]
    local cols = self:_field(f, inp, {})
    if not cols then return nil end
    local len = #cols
    if len <= w then
      local pad = center(f) and (w - len) // 2 or 0
      local out = {}
      for i = 1, w do out[i] = 0 end
      for i = 1, len do out[pad + i] = cols[i] end
      return out
    end
    return cols
  end
  local out, x, any = {}, 0, false
  for i = 1, w do out[i] = 0 end
  for _, f in ipairs(fields) do
    if x >= w then break end
    local fw = f.width == 0 and (w - x) or math.min(f.width, w - x)
    local part = self:_field(f, inp, {})
    if part then
      any = true
      local len = math.min(#part, fw)
      local pad = center(f) and (fw - len) // 2 or 0
      for i = 1, len do out[x + pad + i] = part[i] end
    end
    x = x + fw
  end
  if any then return out end
end

-- Vykreslí jedno pole (šablona + hodnota proměnné) do `out`; nil = prázdné.
function Panel:_field(spec, inp, out)
  local cfg, var = self.cfg, spec.var
  local ok
  if var == "line" then
    ok = self:_line(spec.dop, inp, out)
  elseif var == "stop" then
    local opts = cfg.render
    if inp.request_stop == true then opts = self.opts_on elseif inp.request_stop == false then opts = self.opts_off end
    ok = self:_stop(spec.dop, inp.next_stop, false, opts, out)
  elseif var == "dest" then
    ok = self:_stop(spec.dop, inp.dest, true, self.opts_dest, out)
  elseif var == "linedest" then
    local line = self:_line(DEFAULT_DOP.line, inp, out)
    if line and not is_empty_ref(inp.dest) then
      for _ = 1, cfg.linedest_gap do out[#out + 1] = 0 end
    end
    ok = self:_stop(DEFAULT_DOP.dest, inp.dest, true, self.opts_dest, out) or line
  elseif var == "time" then
    if inp.time_s then
      local t = floor(inp.time_s)
      local digits = string.format("%02d:%02d", t // 3600 % 24, t // 60 % 60)
      -- (databáze bez šablon DOP - verze 1.xx - má „Čas:" ve firmwaru panelu)
      local pre = ""
      if next(self.db.dop) == nil then
        pre = bytes_to_string(E.encode_text(self.db, "Čas: ", { cfg.fallback_stop_font, 0xB1 }))
      end
      E.render_parts(self.db, { self:_dop(spec.dop), pre, digits }, cfg.render, out, self.log)
      ok = true
    end
  elseif var == "zone" then
    ok = self:_text(spec.dop, inp.zone, out)
  elseif var == "info" then
    ok = self:_text(spec.dop, inp.info, out)
  else
    local code = tonumber(tostring(var):match("^unknown:(%d+)$"))
    if code then
      self.log:once("var" .. code, string.format("cyklus: neznámá proměnná stránky %02X, stránka přeskočena", code))
    end
  end
  if ok then return out end
end

local function same_cols(a, b)
  if #a ~= #b then return false end
  for i = 1, #a do
    if a[i] ~= b[i] then return false end
  end
  return true
end

function Panel:_absorb(inp)
  local s, ch = self.seen, 0
  if s.line ~= inp.line then s.line = inp.line ch = ch | CH_LINE end
  if s.dest_id ~= inp.dest.id or s.dest_name ~= inp.dest.name then
    s.dest_id, s.dest_name = inp.dest.id, inp.dest.name
    ch = ch | CH_DEST
  end
  if s.stop_id ~= inp.next_stop.id or s.stop_name ~= inp.next_stop.name then
    s.stop_id, s.stop_name = inp.next_stop.id, inp.next_stop.name
    ch = ch | CH_STOP
  end
  if s.zone ~= inp.zone then s.zone = inp.zone ch = ch | CH_ZONE end
  if s.info ~= inp.info then s.info = inp.info ch = ch | CH_INFO end
  local minute = inp.time_s and floor(inp.time_s) // 60 or nil
  if minute ~= s.minute then s.minute = minute ch = ch | CH_TIME end
  if inp.request_stop ~= s.request then s.request = inp.request_stop ch = ch | CH_STOP end
  if inp.stop_pressed and not s.pressed then ch = ch | CH_PRESS end
  s.pressed = inp.stop_pressed and true or false
  if not s.first then
    -- první snímek není „změna zastávky": panel začíná v klidovém cyklu
    s.first = true
    ch = ch & ~(CH_STOP | CH_PRESS)
  end
  return ch
end

local function enter(row, cfg, mode, idx, spec, slide)
  for i = 1, cfg.width do row.old[i] = row.vis[i] end
  row.mode, row.idx, row.spec, row.valid, row.fresh = mode, idx, spec, true, true
  row.slide_left = (slide and spec.slide and cfg.slide == "push") and cfg.slide_steps or 0
  row.slide_acc, row.scroll_off, row.scroll_acc = 0, 0, 0
  row.scroll_wait, row.passes, row.elapsed = cfg.scroll_start_delay_ms, 0, 0
end

-- Najde první neprázdnou stránku v `mode` od `from` (v klidovém cyklu dokola) a přejde na ni.
function Panel:_goto(row, inp, mode, from, slide)
  local seq = row[mode]
  local n = #seq
  local count = mode == "idle" and n or math.max(n - from + 1, 0)
  for k = 0, count - 1 do
    local idx = mode == "idle" and ((from - 1 + k) % n + 1) or (from + k)
    local spec = seq[idx]
    local cols = self:_build(spec, inp)
    if cols then
      if row.valid and row.mode == mode and row.idx == idx and same_cols(row.cur, cols) then
        row.elapsed, row.passes = 0, 0
      else
        row.cur = cols
        enter(row, self.cfg, mode, idx, spec, slide)
      end
      return true
    end
  end
  return false
end

function Panel:_advance(row, inp)
  local next_idx = row.idx + 1
  local found
  if row.mode == "stop" then
    found = self:_goto(row, inp, "stop", next_idx, true) or self:_goto(row, inp, "idle", 1, true)
  else
    found = self:_goto(row, inp, "idle", next_idx, true)
  end
  if not found and row.valid then
    row.cur = {}
    enter(row, self.cfg, "idle", 1, BLANK, true)
    row.valid = false
  end
end

function Panel:_row_tick(row, dt_ms, changed, inp)
  local cfg = self.cfg
  row.fresh = false
  local trigger = #row.stop > 0
    and ((cfg.stop_on_change and (changed & CH_STOP) ~= 0 and not is_empty_ref(inp.next_stop))
      or (cfg.stop_on_press and (changed & CH_PRESS) ~= 0))
  if not row.started then
    row.started = true
    self:_goto(row, inp, "idle", 1, false)
  elseif trigger and self:_goto(row, inp, "stop", 1, true) then
    -- stránka „zastávka"
  elseif not row.valid then
    if changed ~= 0 then self:_goto(row, inp, "idle", 1, true) end
  elseif (changed & spec_mask(row.spec)) ~= 0 then
    local spec = row.spec
    local cols = self:_build(spec, inp)
    if not cols then
      self:_advance(row, inp)
    elseif not same_cols(cols, row.cur) then
      row.cur = cols
      -- (jen čas: nová minuta se přepíše na místě)
      if (changed & spec_mask(spec) & ~CH_TIME) ~= 0 then
        enter(row, cfg, row.mode, row.idx, spec, true)
      end
    end
  end

  if row.fresh then return end
  if row.slide_left > 0 then
    row.slide_acc = row.slide_acc + dt_ms
    while row.slide_left > 0 and row.slide_acc >= cfg.slide_step_ms do
      row.slide_acc = row.slide_acc - cfg.slide_step_ms
      row.slide_left = row.slide_left - 1
    end
    return
  end
  if not row.valid then return end
  row.elapsed = row.elapsed + dt_ms
  local scrolling = #row.cur > cfg.width
  if scrolling then
    if row.scroll_wait > 0 then
      row.scroll_wait = row.scroll_wait - dt_ms
    else
      local period = #row.cur + cfg.scroll_gap
      row.scroll_acc = row.scroll_acc + dt_ms
      while row.scroll_acc >= cfg.scroll_step_ms do
        row.scroll_acc = row.scroll_acc - cfg.scroll_step_ms
        row.scroll_off = row.scroll_off + 1
        if row.scroll_off >= period then
          row.scroll_off = 0
          row.passes = row.passes + 1
        end
      end
    end
  end
  local dur = row.spec.time_ms == 0 and cfg.hold_ms or row.spec.time_ms
  local pass_done = (not scrolling) or (not cfg.scroll_full_pass) or row.passes >= 1
  local held = row.mode == "stop" and spec_has(row.spec, "stop") and cfg.stop_on_press and inp.stop_pressed
  if row.elapsed >= dur and pass_done and not held then
    self:_advance(row, inp)
  end
end

local function compose(row, cfg)
  local w, cur, vis = cfg.width, row.cur, row.vis
  local len = #cur
  if len <= w then
    -- (obsah stránky je zarovnaný už z _build)
    for x = 1, w do vis[x] = 0 end
    for i = 1, len do vis[i] = cur[i] end
  else
    local period = len + cfg.scroll_gap
    for x = 0, w - 1 do
      local i = (row.scroll_off + x) % period
      vis[x + 1] = i < len and cur[i + 1] or 0
    end
  end
  if row.slide_left > 0 then
    local done = cfg.slide_steps - row.slide_left
    local shift = done * 8 // cfg.slide_steps
    local old = row.old
    for x = 1, w do
      vis[x] = ((old[x] << shift) & 0xFF) | (vis[x] >> (8 - shift))
    end
  end
end

-- Posune čas o `dt` sekund. Vrací snímek jen tehdy, když se od minula změnil (jinak nil).
-- inp = { line=, dest={id=,name=}, next_stop={id=,name=}, stop_pressed=, request_stop=nil|bool,
--         time_s=nil|sekundy, zone=, info= }
function Panel:tick(dt, inp)
  local changed = self:_absorb(inp)
  local dt_ms = 0
  if dt == dt and dt ~= math.huge and dt ~= -math.huge then
    dt_ms = math.max(0, math.min(dt * 1000, 10000))
  end
  local dirty = false
  for r, row in ipairs(self.rows) do
    self:_row_tick(row, dt_ms, changed, inp)
    compose(row, self.cfg)
    local strip, vis = self.frame.strips[r], row.vis
    for x = 1, self.cfg.width do
      if strip[x] ~= vis[x] then
        strip[x] = vis[x]
        dirty = true
      end
    end
  end
  if dirty or not self.emitted then
    self.emitted = true
    self.frame_no = self.frame_no + 1
    return self.frame
  end
end

local HEX = "0123456789ABCDEF"

-- „Nibble row": jeden hex znak na sloupec; hi = horní 4 řádky pruhu, jinak spodní 4.
function E.nibble_row(frame, strip, hi)
  local s, out = frame.strips[strip], {}
  for x = 1, frame.width do
    local v = hi and (s[x] >> 4) or (s[x] & 15)
    out[x] = HEX:sub(v + 1, v + 1)
  end
  return table.concat(out)
end

function E.frame_ascii(frame)
  local parts = {}
  for i, s in ipairs(frame.strips) do parts[i] = E.ascii_art(s, 8) end
  return table.concat(parts, "\n")
end

return E
