# Vnitřní LED panely BUSE – co se dá dohledat (2026-10-02)

Výrobce (buse.cz) rozměry ani rozlišení neuvádí; čísla níže jsou z certifikací dopravních
systémů a z popisů dopravců. U každého údaje je zdroj.

## Jednořádek

| Označení | Body (v × š) | LED Ø / rozteč | Barva | Zdroj |
|---|---|---|---|---|
| BS 120.0A (dříve BS 120.A, BS 120.08 135) | 8 × 135 | 3 / 4,7 mm | červená 625 nm | certifikát KORDIS JMK |
| BS 120 (tramvaje, Ostrava) | 8 × 136 | – | – | mhd-ostrava.cz |
| BS 120 (autobusy, trolejbusy, Ostrava) | 8 × 120 | – | – | mhd-ostrava.cz |

- Staré označení „BS 120.08 135" čte se jako řádky.sloupce: 08 řádků, 135 sloupců.
- Z rozteče 4,7 mm vychází svítící plocha 8 × 135 na **634,5 × 37,6 mm**.
- Kryt jednořádku podle inzerátu: asi 72 × 9 × 6 cm (délka × výška × hloubka).
- Napájení 24 V DC ±10 %, odběr 1,8 A (štítek panelu, forum.omsi.cz).
- Databáze `ADledA.hex` je brněnská (DPMB 2005) a patří k BS 120.0A: v jejím záhlaví je
  číslo posledního sloupce `86` = 134, tedy **135 sloupců** (112 v PROMPT.md je šířka pole
  „cíl" z tabulky cyklů, ne panelu). Rozbor je v `GBUSE_FORMAT.md`.

## Dvouřádek

| Označení | Body (v × š) | Barva | Zdroj |
|---|---|---|---|
| BS 120.0K | 16 × 136 | červená | seznam certifikovaných zařízení PID („Vnitřní dvouřádkový LED 16×136. Textový režim.") |
| BS 120 (Vario LFR.S, LF2R.S, Solaris Trollino 12AC, Ostrava) | 16 × 120 | – | mhd-ostrava.cz |

- Rozteč dvouřádku jsem nenašel. Kdyby byla stejná jako u BS 120.0A (4,7 mm), měla by
  plocha 16 × 136 rozměr 639 × 75 mm – **odhad, ne údaj**.
- Rozložení v Ostravě: horní řádek číslo linky, cílová zastávka, čas a zóna; dolní řádek
  důležité průchozí zastávky, aktuální a příští zastávka.
- Standardy kvality PID (autobusy, 2025-02), bod 4.3.5.2: je-li vnitřní panel LED, musí být
  dvouřádkový.

## BS 190 není dvouřádek

V seznamu PID je **BS 190.0A0A0D „Zobrazovač času a pásma"**, červená LED 10 × 34/24 bodů,
3 alfanumerické znaky pásma (P…98), IBIS + Ethernet. Výrobce má BS 120 a BS 190 na jedné
stránce „LED vnitřní panely", odtud nejspíš záměna. Dvouřádkový panel je BS 120.0K.

## Komunikace

- IBIS: 1200 Bd, 7 datových bitů, sudá parita, 2 stop bity; kontrolní součet = XOR všech
  znaků včetně CR, výsledek XOR 0x7F (pavlik.space). Panel jen přijímá.
- Svorky Wago: SD (povel), MS (zem povelu), ED (odpověď), ME (zem odpovědi), +V, 0V.
- Rozhraní podle výrobce: Ethernet, IBIS, RS485, RS232.
- gBUSE1 dělá databázi pro LED panely, gBUSE0 pro terčové (flip-dot); BSLoader ji nahrává
  jako Intel HEX. Databáze je v každém panelu zvlášť.

## Co se dohledat nepodařilo

- Zarovnání textu (na střed / vlevo) – žádný zdroj; podle uživatele bývá převážně na střed.
- Rozteč a rozměry krytu dvouřádku, výkres, fotky s měřítkem.
- Přesné rozložení řádků u PID verze („pid 2r 16x136").

## Zdroje

- https://kordis-jmk.cz/dopravci/certifikaty/141124Certifik%C3%A1t%20BUSE.pdf
- https://pid.cz/wp-content/uploads/system/sk_bus/ois/SK_bus_Seznam_certifikovanych_zarizeni_BUS_2025-07-14.pdf
- https://pid.cz/wp-content/uploads/system/sk_bus/Standardy_kvality_bus_2025-02.pdf
- https://mhd-ostrava.cz/?s=linka
- https://dp-ostrava.webnode.cz/vozidla-a-buse-panely/
- https://www.buse.cz/en/interior-signs/led-interior-bs-120-signs
- https://pavlik.space/ibis-bs120
- https://forum.omsi.cz/viewtopic.php?f=6&t=10480
- https://aukro.cz/dobry-den-schanim-led-infopanel-buse-bs120-nebo-hlasic-buse-bs200-7006980555
