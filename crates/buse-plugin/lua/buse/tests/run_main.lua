-- run_main.lua - main.lua se stub tabulkou `omsi` (bez hry).
--
--   lua tests/run_main.lua <složka s main.lua, engine.lua, buse_db.lua, config.lua, zst_map.lua>
--
-- Stub drží proměnné vozu v tabulkách a hlídá, že plugin používá jen API z docs/PLUGINS.md.

local dir = PLUGIN_DIR or (arg and arg[1]) or "."
package.path = dir .. "/?.lua;" .. package.path

local vars, strs, sys = {}, {}, { Time = 45000 }
local watches, timers, handlers, logs = {}, {}, {}, {}
local has_vehicle = true

omsi = {
  name = "buse",
  version = "test",
  has_vehicle = function() return has_vehicle end,
  var = function(n) return vars[n] end,
  set_var = function(n, v) vars[n] = v return true end,
  str = function(n) return strs[n] end,
  set_str = function(n, v) strs[n] = v return true end,
  sys = function(n) return sys[n] end,
  watch = function(kind, name, fn)
    if fn == nil then kind, name, fn = "var", kind, name end
    watches[#watches + 1] = { kind = kind, name = name, fn = fn }
    return #watches
  end,
  every = function(secs, fn) timers[#timers + 1] = { secs = secs, fn = fn } return #timers end,
  on = function(event, fn) handlers[event] = fn end,
  log = function(...) logs[#logs + 1] = table.concat({ ... }, " ") end,
  warn = function(...) logs[#logs + 1] = "WARN " .. table.concat({ ... }, " ") end,
}
-- cokoli jiného z `omsi` je chyba testu
setmetatable(omsi, { __index = function(_, k) error("plugin sahá na omsi." .. tostring(k) .. ", které stub nemá") end })

local function frame(dt)
  for _, w in ipairs(watches) do
    local now = (w.kind == "str" and strs or w.kind == "sys" and sys or vars)[w.name]
    if now ~= w.last then
      local old = w.last
      w.last = now
      w.fn(now, old)
    end
  end
  on_frame(dt)
  sys.Time = sys.Time + dt
end

-- vůz: skript v {init} naplní výstupní stringy nulami
local zeros = string.rep("0", 135)
strs.BSLED_r0_hi, strs.BSLED_r0_lo = zeros, zeros
vars.BSLED_frame, vars.elec_busbar_main, vars.haltewunsch = 0, 1, 0
strs.Matrix_Nr, strs.IBIS_terminus_name, strs.IBIS_busstop_name = "53", "Hlavák", "\u{C8}eská"

dofile(dir .. "/main.lua")
if on_start then on_start() end

frame(0.016)
assert(vars.BSLED_frame == 1, "první snímek se zapíše")
assert(#strs.BSLED_r0_hi == 135 and strs.BSLED_r0_lo ~= zeros, "linka 53 svítí")

-- stejný snímek přímo z enginu
local engine = require("engine")
local db = require("buse_db")
local cfg = engine.parse_config(require("config"))
local panel = engine.new_panel(db, cfg)
local inp = {
  line = "53", dest = { id = "1146", name = "Hlavák" }, next_stop = { id = "", name = "Česká" },
  stop_pressed = false, time_s = 45000, zone = "", info = "",
}
local want = panel:tick(0.016, inp)
assert(strs.BSLED_r0_hi == engine.nibble_row(want, 1, true), "hi řádek jako z enginu")
assert(strs.BSLED_r0_lo == engine.nibble_row(want, 1, false), "lo řádek jako z enginu")

-- beze změny se čítač nemění
for _ = 1, 50 do frame(0.016) end
assert(vars.BSLED_frame == 1, "bez změny snímku se nic nezapisuje")

-- STOP -> stránka zastávky; název s opraveným kódováním (Èeská -> Česká)
vars.haltewunsch = 1
frame(0.016)
assert(vars.BSLED_frame == 2, "STOP změní snímek")
local direct = engine.new_panel(db, cfg)
direct:tick(0, inp)
inp.stop_pressed = true
local stop = direct:tick(0.016, inp)
assert(strs.BSLED_r0_lo == engine.nibble_row(stop, 1, false), "stránka zastávky jako z enginu")

-- vypnutý hlavní vypínač -> panel zhasne
vars.elec_busbar_main = 0
frame(0.016)
assert(strs.BSLED_r0_hi == zeros and strs.BSLED_r0_lo == zeros, "bez proudu zhasnuto")

-- bez vozu plugin nic nedělá
has_vehicle = false
local n = vars.BSLED_frame
frame(0.016)
assert(vars.BSLED_frame == n)

assert(#timers == 1 and timers[1].secs == 60, "omsi.every pro stavovou hlášku")
assert(#watches == 2, "omsi.watch pro názvy zastávky a cíle")
for _, l in ipairs(logs) do assert(not l:find("^WARN"), l) end
MAIN_RESULT = { frames = vars.BSLED_frame, logs = logs }
print("main.lua se stub omsi: OK (" .. #logs .. " řádků logu)")
