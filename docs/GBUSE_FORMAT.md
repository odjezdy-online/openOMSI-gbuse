# Formát databází gBUSE (.hex)

Poznámka: soubory, o kterých je řeč (`data/…`), jsou databáze dopravců a v repozitáři nejsou;
tabulka níže říká jen, na čem byl formát ověřen.

Stav 2026-10-02. Všech 26 souborů v `data/` (kromě dvou `Perla*.hex`) čte
`tools/gbuse_decode.py` bez chyby, včetně fontů vnějších panelů. U každého bodu je napsáno,
jestli je **ověřený** (sedí na všech souborech / na vykreslení) nebo **hypotéza**.

## Co je který soubor

| Soubor | Program | Panel (ze záhlaví) | Tabulky |
|---|---|---|---|
| `ADledA.hex`, `ADledB.hex` (+ kopie v `BRNO_2005_09/`) | gBUSE1 1.12 | vnitřní LED 135 × 8 | CYK 7, DOP 7, LIN 337, ZST 693 |
| `KOSICE_2001_01/DISPA.HEX`, `DISPAlol.hex` | gBUSE1 1.00 | vnitřní LED 135 × 8 | LIN, CIL, ZST (bez CYK a DOP) |
| `ADcel0.hex` | gBUSE0 1.17 | čelní 140 × 19 | LIN 336, CIL 1000 |
| `ADbok4.hex` | gBUSE0 1.17 | boční 112 × 19, dvě okna nad sebou | LIN 336, CIL 8, DRU 737 |
| `ADzad7.hex` | gBUSE0 1.17 | zadní (okno linky 28 × 19) | LIN 336 |
| `KOSICE_2001_01/PANEL1..3.HEX` | gBUSE0 0.01 | 140 × 19, 112 × 19, 28 × 19 | LIN, CIL |
| `IDS_BRNO_Adcel`, `teplice`, `the fuck (bukovc)`, `bukovec…`, `ADR_TEXT_PREDNI` | gBUSE0 1.00–1.21 | čelní 140 × 19 | LIN, CIL |
| `OSTRAVA`, `ostravskamuzejninoc`, `Decín MHD`, `TRAM_PRAHA`, `TRS mapa`, `ANTI_001`, `above autobusy…` | gBUSE0 1.00 | 112 × 19 | LIN, CIL |
| `omsi-gge.hex` | gBUSE0 1.21 | 135 × 19 | LIN 2, CIL 17 |
| `OstravskaMuzejniNoc2017.hex` | gBUSE0 1.00, jiná hlavička (typ 55) | 112 × 19 | LIN, CIL; fonty se nenačtou |
| `PerlaD.hex`, `PerlaE.hex` | gBUSE2b 0.01 | ? | jen prázdná sekce FNT |
| `font.fnt`, `KOSICE_2001_01/BS100.BBF` | – | – | nerozebráno |

Jméno databáze v záhlaví (`T0A0011_050603`) nese adresu panelu na sběrnici IBIS: `T0A`/`T0B`
vnitřní panely A a B, `T00` čelo, `T04` bok, `T07` záď – odtud jména `ADledA`, `ADcel0`,
`ADbok4`, `ADzad7`.

## Kontejner – ověřeno

- Intel HEX → souvislý obraz od adresy 0, nevyplněné bajty `FF`.
- Sekce: 16B ASCII hlavička `XXX: gBUSEn - verze` na adrese dělitelné 32, data na další 32B
  hranici, konec = `FF` na místě dalšího záznamu.
- Tabulky `LIN`, `CIL`: `[id 3 ASCII][délka LE16][text … 0D]`; `ZST`, `DRU`: id 4 znaky. Výjimka:
  `CIL` v `ADR_TEXT_PREDNI.hex` (gBUSE0 1.21) má id 4 znaky (`0000`, `9000`, `XXXX`). Dekodér
  délku id zjistí sám (ta, se kterou řetěz záznamů dojde čistě k `FF`).
- `CYK`, `DOP`: `[id 1 B][délka LE16][data]`.

## Záhlaví obrazu (0x00–0x3F)

### gBUSE1 (první bajt `AA` nebo `AB`)

```
AB 00 86 81 F3 81 E3 01 0A | 02 00 | 15 00 | 00 3F | 00 3F | 23 00 | 00 3F | 00 60 | 01 A0 | …   Brno 1.12
AA 00 86 87 86    E3 01 0A | 00 60 | 13 00 | 19 60 | 30 40 | 00 00 …                           Košice 1.00
```

- Bajt 2 = **číslo posledního sloupce**: `86` = 134 → panel má **135 sloupců**. Ověřeno
  nezávisle: BS 120.0A má podle certifikátu IDS JMK 8 × 135 bodů (viz
  `BUSE_panely_specifikace.md`). Engine si odtud bere šířku (`width = auto`).
- `E3 01 0A`: výchozí font E3, mezera 1 (třetí bajt neznámý).
- Dál ukazatele na data sekcí, big-endian: Brno FNT `0200`, LIN `1500`, ZST `2300`, CYK `0060`,
  DOP `01A0`; `00 3F` = sekce chybí. Košice FNT `0060`, LIN `1300`, CIL `1960`, ZST `3040`.
- Bajty `81 F3 81` (Brno) / `87 86` (Košice) a `01 01 02` za ukazateli: **neznámé**.

### gBUSE0 (první bajt `57`, vnější panely)

```
57 | 00 12 00 1B | 00 12 1C 8B | 45 00 F1 00 | E8 01 00 0A | E3 01 00 0A | 00 80 | 6E 20 | 7E C0 | …
     okno linky    okno cíle                    font+mezera   font+mezera   FNT     LIN     CIL
```

- Okna jsou `[y0 y1 x0 x1]` (včetně): linka řádky 0–18, sloupce 0–27; cíl řádky 0–18, sloupce
  28–139 (čelo `8B`) nebo 28–111 (bok, záď `6F`). Sedí na všech souborech: rozměr panelu
  vychází 140 × 19, 112 × 19, 28 × 19 – stejně jako terčové panely BS 210 (19 × 140, 19 × 112,
  19 × 28).
- Boční `ADbok4` má okno cíle jen do řádku 8 a za jménem databáze třetí okno
  `09 12 1C 6F | E1 01 00 0A | 7F A0`: řádky 9–18, font E1 a ukazatel na tabulku **DRU**
  (`-Achtelky-`, `-Akátky-` …: nácestné zastávky pod cílem).
- `E8 01 00 0A` / `E3 01 00 0A`: výchozí font okna linky (E8) a cíle (E3), mezera 1.

## Fonty (FNT) – ověřeno

`[id E0..EF][délka BE16][glyfy…]`, glyf = `[kód][počet bajtů dat][výška][data]`.

- Sloupec má `ceil(výška / 8)` bajtů, první bajt = horní řádky, bity zarovnané nahoru
  (MSB = horní řádek). **Druhý bajt je počet bajtů, ne šířka**: u fontů výšky 8 (gBUSE1) to
  vyjde nastejno, u vyšších je šířka = počet bajtů / bajty na sloupec. Tím se čtou i fonty
  gBUSE0 (výšky 9, 10, 16, 19, 28) – ověřeno vykreslením („Brno 12", „105", „Bystrc").
- gBUSE1 1.00 (Košice) ukončuje seznam glyfů dvojicí `FF 00`.
- Za posledním fontem gBUSE1 je tabulka kód → Windows-1250 (index 0 = kód 0x20).
- gBUSE1 Brno: fonty E0–E3 výšky 8. Košice navíc `EF` (1 glyf) a E2 má glyfy výšky 9.
- gBUSE0: E0–E3 malé (9–10 řádků, dvouřádkové texty), E4–E8 velké (16–19), E7/E8 i 28
  řádků, E9–EF piktogramy a číslice linek.

## Řídicí kódy v textu

| Bajt | Význam | Jistota |
|---|---|---|
| `0D` | konec textu | ověřeno |
| `E0`–`EF` | font | ověřeno |
| `B0`–`BF` | mezera mezi znaky = kód − B0 sloupců | ověřeno |
| `C0`–`CF` | **svislý posun** = kód − C0 řádků: v gBUSE0 dvouřádkové cíle `E3 B2 C0 horní 0A E3 B2 CA dolní` (`CA` = o 10 řádků níž), četnosti C0 4396×, CA 966×, C9 306×, C1 149× | gBUSE0 ověřeno vzorem, v gBUSE1 hypotéza |
| `0A`, `0C` | nový řádek, nová stránka (gBUSE0) | ověřeno vzorem |
| `&` (26) | v ZST gBUSE1 na začátku každé zastávky; brněnské LED fonty glyf nemají → nic se nekreslí. V Košicích a v gBUSE0 je `&` normální glyf | ověřeno |
| `1B x` | povely vnějších panelů, viz „Vykreslování vnějšího panelu" níže | ověřeno v gBUSE0.exe |

### `C0` není „zastávka na znamení"

Původní domněnka (PROMPT.md 2.4) byla, že `C0` v ZST označuje zastávku na znamení a `&` je
místo pro její symbol. Data to nepotvrzují:

- stejné kódy `C0`–`CF` jsou ve vnějších panelech svislý posun textu;
- `C0` mají i velké přestupní zastávky (Koliště, Olomoucká, Řečkovice), nemají ho malé
  (Ořešín, Měnín);
- `C0` jde vždy ruku v ruce s mezerou za `&` (473 ze 474), tedy spíš s tím, kterou verzí
  editoru záznam vznikl.

Engine proto `&` ve výchozím stavu nekreslí a `C0` ignoruje. Symbol jde zapnout
(`placeholder = symbol:E1:F1`, `placeholder_flag = C0`).

## Cykly (CYK, jen Brno) – nově přečtené

`[00 00][položky…][FF][00 00][jména položek jako Pascal stringy]`. Položka je buď **pole**
(5 B), nebo samotný bajt `00` = **konec stránky** (má prázdné jméno).

```
pole: [šířka ve sloupcích][DOP šablona][proměnná][režim][doba]
```

| Cyklus | Položky | Jména |
|---|---|---|
| 0, 1, 3, 4 | `16 05 01 03 04`, `70 02 09 03 04`, `87 07 0A 03 00`, `00` | linka, cil, mim.inf., „" |
| 2, 5 | `43 04 0E 00 04`, `43 03 0D 00 04` | zóna, čas |
| 6 | `87 01 08 03 04`, `00`, `87 07 0A 03 00`, `00` | zastávka, „", mim.inf., „" |

- **Šířka** (ověřeno daty): `16` = 22, `70` = 112, `43` = 67, `87` = 135. Pole stojí vedle
  sebe, dokud se vejdou do 135 sloupců: **linka (22) + cíl (112)** se zobrazují současně,
  stejně **zóna (67) + čas (67)**; zastávka a mimořádná informace mají celý panel. Nejširší
  linka v LIN má přesně 22 sloupců, nejširší název se šipkou z DOP 2 se vejde do 112.
  Původní čtení („bajt 0 = flags, bit 7 = šestibajtový záznam") byla shoda okolností:
  `87` je šířka 135 a „šestý bajt" je položka konec stránky.
- **Proměnná**: 01 linka (LIN), 08 příští zastávka, 09 cíl, 0A mimořádná informace, 0D čas,
  0E zóna – hypotéza, sedí se jmény položek.
- **Režim**: `03` u linky, cíle, zastávky a informace, `00` u zóny a času. Zarovnání to není:
  podle náhledu v gBUSE1 stojí **všechno na střed svého pole**, i zóna a čas. Engine bere
  nenulový režim jako „stránka se nasouvá"; skutečný význam je neznámý.
- **Doba**: `04` = 4 s, `00` = do odvolání – hypotéza.

DOP šablony: 1 = značka zastávky + mezera, 2 = šipka, 3 = „Čas:", 4 = „Zóna:", 5–7 = jen font.

## Verze dat gBUSE1: 1.xx a 2.00

Náhled v gBUSE1 ukazuje u Košic `[1.xx]`, u Brna `[2.00]` (první bajt obrazu `AA` / `AB`).

- **1.xx** (Košice): bez CYK a DOP. Panel má tři pevné stránky - *linka + cíl*, *pásmo + čas*,
  *zastávka* - a cíle ve vlastní tabulce CIL (id 3 číslice). „Čas:" je ve firmwaru panelu.
- **2.00** (Brno): stránky popisuje CYK, předpony DOP, cíl se bere ze ZST.
- Obě verze umí místo čísla volný text (v gBUSE1 pole pod náhledem: `zl` cíl, `zM`
  mimořádná informace, `v` zastávka) - tak se kreslí i názvy ze hry, které v databázi nejsou.

Engine: bez CYK použije stránky `fallback_pages = line+dest:4, zone+time:4` a pro cíl hledá
nejdřív v CIL.

## Vykreslování vnějšího panelu (z gBUSE0.exe)

Popsáno podle chování náhledu v editoru gBUSE0 (vlastními slovy; z programu se nic nekopíruje).
Implementace: `crates/buse-engine/src/outer.rs`.

- **Pole**: linka, cíl, zastávka. Každé má v konfiguraci okno `[dolní, horní, levý, pravý]`;
  řádky se počítají **odspodu** (dolní 0, horní 18 = všech 19 řádků). Boční panel: cíl má
  dolní 0 / horní 8 (spodních 9 řádků), zastávka dolní 9 / horní 18 (horních 10).
- **Řádek textu** se kreslí do pomocného bufferu od sloupce 0: glyf na svislé poloze `y`
  (`C0..DF` = 0..31 řádků od horního okraje okna), mezi glyfy mezera `B0..BF`.
- **Konec řádku** (`0A`, `0C`, `0D`): řádek se přenese do okna **vodorovně na střed**
  (`(šířka okna - šířka textu) / 2`, širší text od levého okraje). Přepíše jen řádky, které
  text zabral, takže dvouřádkový cíl jsou dva nezávisle středěné řádky.
- `0A`: první v textu je obyčejný nový řádek; **každé další znamená krok animace** (panel
  počká a kreslí dál), `0B` skočí zpět za první `0A` (smyčka). `0C` = další úsek bez čekání.

| Povel | Význam |
|---|---|
| `ESC l n`, `ESC p n` | levý / pravý okraj okna = n − 16 (`1B 6C 10` = sloupec 0, `1B 70 9B` = sloupec 139) |
| `ESC h n`, `ESC d n` | horní / dolní okraj okna = n − 16 (`1B 68 22` = řádek 18) |
| `ESC c` | okno zpět podle konfigurace |
| `ESC s n` | text začne ve sloupci n − 16 místo středění |
| `ESC i` | inverze okna (`… 0C 1B 69` = inverzní nápis; v LIN 999 svítící blok) |
| `ESC o` | smazání okna (od verze dat 5) |
| `ESC b` | text cíle zabere celý panel, linka a zastávka se nekreslí |
| `ESC w` | text cíle zabere i pole zastávky |
| `ESC x` | příští řádek se přenese jen rozsvícenými body (přes to, co už svítí) |
| `ESC n`, `ESC r` | posun obsahu okna o řádek nahoru / o sloupec vlevo (engine je zatím nedělá) |
| `ESC z n` | n − 16; engine tím násobí dobu kroku animace (hypotéza) |
| `ESC a n`, `ESC v` | bez účinku na kreslení |
| `ESC 20..2F`, `30..4F`, `50..5F` | sedmibitové obdoby `B0..`, `C0..`, `E0..` (mezera, poloha, font) |

Záhlaví: bajty 0x0D–0x10 a 0x11–0x14 jsou „formátovací řetězce" pole linky a cíle
(font, mezera, poloha prvního a druhého řádku: `E8 01 00 0A`, `E3 01 00 0A`), 0x2F–0x32 okno
zastávky, 0x33–0x36 jeho formát, 0x37–0x38 ukazatel na DRU. Formát platí pro volný text po
IBIS (`zA`, `zI`, `aA` cíl, `zN` nácestné zastávky, `l` linka).

Texty jsou v kódování Kamenických; velké Í mají databáze vnějších panelů na kódu `7F`.

## Co zůstává otevřené

- Doba kroku animace vnějších panelů (`outer_step_ms`, výchozí 2 s) a význam `ESC z`.
- Neznámé bajty záhlaví gBUSE1 (`81 F3 81`, `01 01 02`).
- Fonty E5–E9 v brněnském ZST (`E8 F8` u nádraží, `E9 F8` u letiště, `E7 F8` u divadla,
  `E5 F8` u myslivny): v databázi nejsou, engine je mapuje na piktogramy E1 (`pict_alias`).
- `OstravskaMuzejniNoc2017.hex` (hlavička typu 55), `font.fnt`, `BS100.BBF`, `Perla*.hex`.
