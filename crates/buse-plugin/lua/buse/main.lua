-- plugins/buse/main.lua - vnitřní LED panel BUSE BS120 / BS190 pro openOMSI (Lua plugin).
--
-- Dělá totéž co buse_panel.dll: čte vstupy z proměnných vozu, počítá panel (engine.lua)
-- a snímek zapisuje do string proměnných <prefix>_r*_hi / _lo a čítače <prefix>_frame.
-- Skript vozu (buse_panel.osc) a model jsou společné s DLL variantou.
--
-- Soubory ve složce pluginu (generuje gbuse-convert --lua):
--   buse_db.lua     databáze gBUSE1 jako Lua tabulka (Lua v openOMSI nemá io)
--   config.lua      text buse_panel.cfg (return [==[ ... ]==])
--   zst_map.lua     normalizovaný název -> ZST id (volitelné)

local engine = require("engine")
local db = require("buse_db")

local ok_cfg, cfg_text = pcall(require, "config")
local cfg, warnings = engine.parse_config(ok_cfg and cfg_text or "")
for _, w in ipairs(warnings) do omsi.warn("buse_panel.cfg: " .. w) end
local ok_map, zst_map = pcall(require, "zst_map")
if not ok_map then zst_map = {} end

local extra = cfg.extra

-- Zdroj vstupu: "str:JMENO", "num:JMENO" nebo "num:JMENO/DELITEL"; nil = nepoužít.
local function source(key, default)
  local v = extra[key]
  if v == nil then v = default end
  if v == nil or v == "" or v:lower() == "none" then return nil end
  local kind, rest = v:match("^%s*(%a+)%s*:%s*(.-)%s*$")
  if kind == "str" then return { kind = "str", name = rest } end
  if kind == "num" then
    local name, div = rest:match("^(.-)%s*/%s*(.+)$")
    return { kind = "num", name = name or rest, div = tonumber(div) or 1 }
  end
  omsi.warn("buse_panel.cfg: " .. key .. ": čekám str:JMENO nebo num:JMENO")
end

local src = {
  line = source("in_line", "str:Matrix_Nr"),
  dest_name = source("in_dest_name", "str:IBIS_terminus_name"),
  dest_code = source("in_dest_code"),
  stop_name = source("in_stop_name", "str:IBIS_busstop_name"),
  stop_code = source("in_stop_code"),
  stop_pressed = source("in_stop_pressed", "num:haltewunsch"),
  request = source("in_request"),
  zone = source("in_zone"),
  info = source("in_info"),
  power = source("in_power", "num:elec_busbar_main"),
}
local out_frame = extra.out_frame or "BSLED_frame"
local out_prefix = extra.out_prefix or "BSLED_r"
local fix_encoding = (extra.fix_encoding or "auto"):lower() ~= "off"

local panel = engine.new_panel(db, cfg)
local frame_no, blank, fed = 0, false, false
local zeros = string.rep("0", cfg.width)

-- Text vstupu (string proměnná, nebo číslo jako text; 0 = nic).
local function text(s)
  if not s then return "" end
  if s.kind == "str" then
    local v = omsi.str(s.name) or ""
    v = engine.trim(v)
    return fix_encoding and engine.fix_mojibake(v) or v
  end
  local v = (omsi.var(s.name) or 0) / s.div
  return v >= 1 and tostring(math.floor(v)) or ""
end

local function number(s)
  if not s then return 0 end
  return (omsi.var(s.name) or 0) / (s.div or 1)
end

-- Názvy zastávky a cíle se mění zřídka: mapování na ZST id se počítá jen při změně.
local names = { dest = { text = "", id = "" }, stop = { text = "", id = "" } }
local function watch_name(slot, s)
  if not s or s.kind ~= "str" then return end
  omsi.watch("str", s.name, function(new)
    local v = engine.trim(new or "")
    if fix_encoding then v = engine.fix_mojibake(v) end
    names[slot].text = v
    names[slot].id = zst_map[engine.normalize(v)] or ""
  end)
end
watch_name("dest", src.dest_name)
watch_name("stop", src.stop_name)

local function flush_log()
  for _, m in ipairs(panel.log:take()) do omsi.log(m) end
end

local function write_frame(frame)
  for strip = 1, #frame.strips do
    local base = out_prefix .. (strip - 1)
    omsi.set_str(base .. "_hi", blank and zeros or engine.nibble_row(frame, strip, true))
    omsi.set_str(base .. "_lo", blank and zeros or engine.nibble_row(frame, strip, false))
  end
  frame_no = frame_no + 1
  if frame_no > 1e6 then frame_no = 1 end
  omsi.set_var(out_frame, frame_no)
end

omsi.on("vehicle", function()
  -- nový vůz: panel začne znovu od klidového cyklu a první snímek se zapíše celý
  panel = engine.new_panel(db, cfg)
  fed = false
  names.dest.text, names.dest.id, names.stop.text, names.stop.id = "", "", "", ""
end)

function on_frame(dt)
  if not omsi.has_vehicle() then return end
  local stop_text = src.stop_name and (src.stop_name.kind == "str" and names.stop.text or text(src.stop_name)) or ""
  local dest_text = src.dest_name and (src.dest_name.kind == "str" and names.dest.text or text(src.dest_name)) or ""
  local stop_id, dest_id = names.stop.id, names.dest.id
  if stop_id == "" then stop_id = text(src.stop_code) end
  if dest_id == "" then dest_id = text(src.dest_code) end
  local zone, info = text(src.zone), text(src.info)
  local inp = {
    line = text(src.line),
    dest = { id = dest_id, name = dest_text },
    next_stop = { id = stop_id, name = stop_text },
    stop_pressed = number(src.stop_pressed) > 0.5,
    request_stop = src.request and (number(src.request) > 0.5) or nil,
    time_s = omsi.sys("Time"),
    zone = zone ~= "" and zone or (extra.zone_text or ""),
    info = info ~= "" and info or (extra.info_text or ""),
  }
  local now_blank = src.power ~= nil and number(src.power) < 0.5
  local changed = now_blank ~= blank or not fed
  blank, fed = now_blank, true
  if panel:tick(dt, inp) then changed = true end
  if changed then write_frame(panel.frame) end
  if #panel.log.msgs > 0 then flush_log() end
end

omsi.every(60, function()
  if omsi.has_vehicle() then
    omsi.log(string.format("BUSE panel: %d snímků, databáze %s", frame_no, db.name))
  end
end)

function on_start()
  omsi.log(string.format("BUSE panel %dx%d, databáze %s (%d B)", cfg.width, cfg.rows * 8, db.name, db.size))
  flush_log()
end
