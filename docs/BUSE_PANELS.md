# BUSE panels

This repository is openOMSI with the BUSE information panels built in: the inner LED panel
(BS 120, one or two rows) and the outer front, side and rear panels (BS 210 and alike), fed
by the databases the gBUSE editors write (`.hex`, Intel HEX). What they show - line,
destination, next stop, via stops, time and zone, with the fonts, cycles and animations of
the database - is worked out by `buse-engine`; the bus's script copies the finished frame
into a script texture.

## What is where

| | |
|---|---|
| `crates/buse-engine` | the panels themselves: database reader, text rendering, the inner panel's page cycles, the outer panels' windows and escape sequences. No I/O, no dependencies. |
| `crates/buse-plugin` | reads a panel folder (`buse_panel.cfg`, databases, name map) and maps a bus's variables to the engine's inputs. A library for the game (`buse_panel::core`) and an OMSI 2 plugin DLL (`buse_panel.dll` + `.opl`). `lua/buse` is a Lua port of the inner panel for openOMSI builds without this integration. |
| `crates/buse-tools` | `gbuse-convert` (database + bus -> plugin folder, script, fonts, model additions), `hof2hex` (a map's `.hof` -> databases), `buse-preview`, `buse-outer`, `buse-sim` (terminal previews). |
| `crates/omsi-app/src/buse.rs` | the game's side: finds the plugin's `.opl` under `plugins`, does not load its DLL, and drives the panels of that folder itself each frame. |
| `docs/GBUSE_FORMAT.md` | the database format as far as it is known (Czech). |
| `docs/BUSE_PANEL_SPECS.md` | sizes and dot pitch of the real panels, with sources (Czech). |
| `tools/gbuse/gbuse_decode.py` | a standalone decoder of a database to JSON and PNG. |

## How a bus gets its panels

1. `gbuse-convert <database.hex> -o out --vehicle <bus.bus> …` writes a tree laid out like
   the game folder: `plugins/<name>/` (configuration, database, `.opl`), `Fonts/BUSE_nib.*`
   and a copy of the bus with the panel's script, script texture and meshes. Run it again
   on that copy for each further panel. `hof2hex` makes databases from a map's `.hof` first
   when there are none for the map.
2. Copy the tree into the content folder (or the OMSI 2 folder).
3. openOMSI built from this repository needs nothing else: `buse.rs` sees the `.opl` whose
   `[dll]` begins with `buse_panel`, reads the lists of variables from it and the panels
   from its folder. A `.hex` or `buse_panel.cfg` saved while the game runs is taken up
   within two seconds.
4. OMSI 2 (32-bit) needs the DLL beside the `.opl`:
   `cargo build --release -p buse-plugin --target i686-pc-windows-msvc`.

One plugin folder may hold several panels: the folder itself and every subfolder with a
`buse_panel.cfg` is a panel, all sharing the `.opl`'s lists.

## The databases are not here

The `.hex` files belong to the operators they were made for and are not part of this
repository; neither is anything from the gBUSE programs. Bring your own. The tests that
need databases are behind a feature:

```
BUSE_DATA=/folder/with/data-and-reference cargo test -p buse-engine -p buse-plugin -p buse-tools \
  --features buse-engine/buse-data,buse-plugin/buse-data,buse-tools/buse-data
```

`BUSE_DATA` names a folder with `data/ADledA.hex`, `data/ADcel0.hex`, … and
`reference/golden_render.json`, `reference/ADledA.json` (made by `gbuse_decode.py`).

## State

Works: the inner panel (cycles, scrolling, push effect, two rows) and the outer panels
(windows, fonts, animation steps). Seen in the game built from this repository on a SOR
NB 12 with databases made by `hof2hex`: the panels are driven by `buse.rs`, the plugin's DLL
is not loaded. Not done yet: drawing into a bus's existing matrix texture (the panels are
separate meshes so far), databases with the fonts of the bus's old matrix, and the Lua
port of the outer panels. The databases `hof2hex` writes have not been opened in the gBUSE
editors themselves, and the DLL has not been tried in OMSI 2 since several panels share one
library.
