-- run_sim.lua - Lua engine proti Rust enginu, snímek po snímku.
--
--   lua tests/run_sim.lua <buse_db.lua> <sim_cases.lua>
--
-- sim_cases.lua generuje `cargo test -p buse-tools --test lua` z Rust enginu:
--   return { { name=, cfg="text konfigurace", steps={ { dt=, line=, dest_id=, dest_name=,
--     stop_id=, stop_name=, pressed=, request=, time=, zone=, info=, hi={..}, lo={..} }, ... } } }
-- `hi`/`lo` jsou nibble rows snímku, který Rust v tom kroku vydal (nil = beze změny).

local here = (arg and arg[0] or ""):match("^(.*)[/\\]tests[/\\][^/\\]*$")
if here then package.path = here .. "/?.lua;" .. package.path end

local engine = require("engine")
local db = assert(loadfile(BUSE_DB_PATH or (arg and arg[1]) or "buse_db.lua"))()
local cases = assert(loadfile(SIM_PATH or (arg and arg[2]) or "sim_cases.lua"))()

local total, frames = 0, 0
for _, case in ipairs(cases) do
  local cfg, warn = engine.parse_config(case.cfg)
  assert(#warn == 0, case.name .. ": " .. table.concat(warn, "; "))
  local panel = engine.new_panel(db, cfg)
  for i, s in ipairs(case.steps) do
    local frame = panel:tick(s.dt, {
      line = s.line,
      dest = { id = s.dest_id, name = s.dest_name },
      next_stop = { id = s.stop_id, name = s.stop_name },
      stop_pressed = s.pressed,
      request_stop = s.request,
      time_s = s.time,
      zone = s.zone,
      info = s.info,
    })
    total = total + 1
    if (frame ~= nil) ~= (s.hi ~= nil) then
      error(string.format("%s, krok %d: Lua %s snímek, Rust %s", case.name, i,
        frame and "vydala" or "nevydala", s.hi and "ano" or "ne"))
    end
    if frame then
      frames = frames + 1
      for strip = 1, #frame.strips do
        local hi, lo = engine.nibble_row(frame, strip, true), engine.nibble_row(frame, strip, false)
        if hi ~= s.hi[strip] or lo ~= s.lo[strip] then
          error(string.format("%s, krok %d, pruh %d: snímek se liší\nLua:\n%s", case.name, i, strip, engine.frame_ascii(frame)))
        end
      end
    end
  end
  local msgs = panel.log:take()
  local want = case.log or {}
  assert(#msgs == #want, case.name .. ": hlášky enginu: " .. table.concat(msgs, " | ") .. " vs " .. table.concat(want, " | "))
  for i = 1, #msgs do
    assert(msgs[i] == want[i], case.name .. ": hláška " .. msgs[i] .. " vs " .. want[i])
  end
end

SIM_RESULT = { cases = #cases, steps = total, frames = frames }
print(string.format("sim (Lua vs Rust): %d scénářů, %d kroků, %d snímků - shodné", #cases, total, frames))
