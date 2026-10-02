-- run_golden.lua - golden test Lua portu enginu, spustitelný mimo hru v čisté Lua 5.4:
--
--   lua tests/run_golden.lua <buse_db.lua> <golden_render.json>
--
-- (cesty lze předat i globály BUSE_DB_PATH / GOLDEN_PATH; tak ho pouští `cargo test`).
-- Všech 1037 položek z reference/golden_render.json musí sedět bit po bitu.

local here = (arg and arg[0] or ""):match("^(.*)[/\\]tests[/\\][^/\\]*$")
if here then package.path = here .. "/?.lua;" .. package.path end

local db_path = BUSE_DB_PATH or (arg and arg[1]) or "buse_db.lua"
local golden_path = GOLDEN_PATH or (arg and arg[2]) or "golden_render.json"

local engine = require("engine")
local db = assert(loadfile(db_path))()

-- Minimální JSON parser (objekty, pole, řetězce, čísla, true/false/null).
local function json_decode(s)
  local pos = 1
  local value
  local function ws()
    pos = s:find("%S", pos) or #s + 1
  end
  local function str()
    local out = {}
    pos = pos + 1
    while true do
      local c = s:sub(pos, pos)
      if c == '"' then
        pos = pos + 1
        return table.concat(out)
      elseif c == "\\" then
        local e = s:sub(pos + 1, pos + 1)
        if e == "u" then
          out[#out + 1] = utf8.char(tonumber(s:sub(pos + 2, pos + 5), 16))
          pos = pos + 6
        else
          out[#out + 1] = ({ n = "\n", t = "\t", r = "\r", b = "\b", f = "\f" })[e] or e
          pos = pos + 2
        end
      else
        local stop = s:find('["\\]', pos)
        out[#out + 1] = s:sub(pos, stop - 1)
        pos = stop
      end
    end
  end
  function value()
    ws()
    local c = s:sub(pos, pos)
    if c == "{" then
      local t = {}
      pos = pos + 1
      ws()
      if s:sub(pos, pos) == "}" then pos = pos + 1 return t end
      while true do
        ws()
        local k = str()
        ws()
        pos = pos + 1 -- ':'
        t[k] = value()
        ws()
        local d = s:sub(pos, pos)
        pos = pos + 1
        if d == "}" then return t end
      end
    elseif c == "[" then
      local t = {}
      pos = pos + 1
      ws()
      if s:sub(pos, pos) == "]" then pos = pos + 1 return t end
      while true do
        t[#t + 1] = value()
        ws()
        local d = s:sub(pos, pos)
        pos = pos + 1
        if d == "]" then return t end
      end
    elseif c == '"' then
      return str()
    else
      local lit = s:match("^[%w%.%-%+]+", pos)
      pos = pos + #lit
      if lit == "true" then return true elseif lit == "false" then return false elseif lit == "null" then return nil end
      return tonumber(lit)
    end
  end
  return value()
end

local f = assert(io.open(golden_path, "rb"))
local gold = json_decode(f:read("a"))
f:close()

local function unhex(s)
  local out = {}
  for b in s:gmatch("%x%x") do out[#out + 1] = string.char(tonumber(b, 16)) end
  return table.concat(out)
end

local opts = engine.reference_render()
local log = engine.new_log()
local bad = {}
for _, item in ipairs(gold.items) do
  local cols = engine.render_text(db, unhex(item.raw), opts, log)
  local ok = #cols == #item.cols
  for i = 1, #cols do
    if cols[i] ~= item.cols[i] then ok = false break end
  end
  if not ok then bad[#bad + 1] = item.table .. " " .. item.id end
end

-- záznamy v buse_db.lua musí být tytéž jako v golden souboru
local count = 0
for _, item in ipairs(gold.items) do
  local tab = ({ LIN = db.lin, ZST = db.zst })[item.table]
  local raw = tab and tab[item.id] or db.dop[tonumber(item.id)]
  if raw ~= unhex(item.raw) then bad[#bad + 1] = "db " .. item.table .. " " .. item.id end
  count = count + 1
end

GOLDEN_RESULT = { total = count, bad = bad, log = log:take() }
print(string.format("golden (Lua): %d/%d sedí", count - #bad, count))
for i = 1, math.min(#bad, 10) do print("  nesedí: " .. bad[i]) end
if #bad > 0 or count ~= 1037 then error("golden test selhal") end
