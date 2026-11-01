-- The global library of the original's Lua 3.2.1 state, rebuilt on Lua 5.2.
-- CLua::CLua (debug build 0x16027c0) calls lua_open + lua_iolibopen + lua_strlibopen; there is
-- NO mathlib (scripts that need sqrt define their own). Names below are exactly the ones in the
-- shipped exe's builtin tables (base 0x1169758, strlib 0x11696c0, iolib 0x11695f8/0x1169658).
-- `__host_trace` is installed by the engine before this chunk runs.
local trace = __host_trace
__host_trace = nil
local string, table, os, math = string, table, os, math
local type, pairs, next, rawget, rawset, tostring, tonumber, unpack, load, error =
      type, pairs, next, rawget, rawset, tostring, tonumber, table.unpack, load, error
local G = _G

-- strlib (3.2 names)
strlen   = string.len
strsub   = string.sub
strlower = string.lower
strupper = string.upper
strchar  = string.char
strrep   = string.rep
function strbyte(s, i) return string.byte(s, i or 1) end
ascii    = strbyte
format   = string.format
strfind  = string.find
gsub     = string.gsub

-- base library: 3.2 table helpers keep the size in field "n"
function getn(t)
  local n = rawget(t, "n")
  if type(n) == "number" then return n end
  local max = 0
  for k, _ in pairs(t) do
    if type(k) == "number" and k > max and math.floor(k) == k then max = k end
  end
  return max
end

function tinsert(t, a, b)
  local n = getn(t)
  local pos, v
  if b == nil then pos, v = n + 1, a else pos, v = a, b end
  t.n = n + 1
  local i = n
  while i >= pos do t[i + 1] = t[i]; i = i - 1 end
  t[pos] = v
end

function tremove(t, pos)
  local n = getn(t)
  if n <= 0 then return nil end
  pos = pos or n
  local v = t[pos]
  while pos < n do t[pos] = t[pos + 1]; pos = pos + 1 end
  t[n] = nil
  t.n = n - 1
  return v
end

function sort(t, comp)
  local n, a = getn(t), {}
  for i = 1, n do a[i] = t[i] end
  if comp then table.sort(a, function(x, y) return comp(x, y) and true or false end)
  else table.sort(a) end
  for i = 1, n do t[i] = a[i] end
end

function foreach(t, f)
  for k, v in pairs(t) do local r = f(k, v); if r ~= nil then return r end end
end

function foreachi(t, f)
  for i = 1, getn(t) do local r = f(i, t[i]); if r ~= nil then return r end end
end

function foreachvar(f)
  for k, v in pairs(G) do local r = f(k, v); if r ~= nil then return r end end
end

function nextvar(name) return next(G, name) end
rawgettable = rawget
rawsettable = rawset
function rawgetglobal(n) return rawget(G, n) end
function rawsetglobal(n, v) rawset(G, n, v) end
function getglobal(n) return G[n] end
function setglobal(n, v) G[n] = v end

function call(f, args) return f(unpack(args, 1, getn(args))) end

function dostring(s)
  local chunk, err = load(s)
  if not chunk then trace("SCRIPT: dostring: " .. tostring(err)); return nil end
  return chunk()
end

-- tag methods / error method: unused by the shipped scripts; kept as inert stand-ins.
local tagcount = 0
function newtag() tagcount = tagcount + 1; return tagcount end
function settag(o) return o end
function tag() return 0 end
function settagmethod() return nil end
function gettagmethod() return nil end
function copytagmethods() return nil end
local errormethod
function seterrormethod(f) local old = errormethod; errormethod = f; return old end
function _ALERT(s) trace(tostring(s)) end
function print(...)
  local parts = {}
  for i = 1, select("#", ...) do parts[i] = tostring((select(i, ...))) end
  trace(table.concat(parts, "\t"))
end

-- iolib: the original could touch the file system; here only the harmless parts work.
function write(...)
  for i = 1, select("#", ...) do trace(tostring((select(i, ...)))) end
end
function clock() return os.clock() end
function date(f) return os.date(f) end
local function nothing() return nil end
readfrom, writeto, appendto, closefile, openfile, read, seek, flush = nothing, nothing, nothing, nothing, nothing, nothing, nothing, nothing
remove, rename, tmpname, getenv, execute, setlocale, debug = nothing, nothing, nothing, nothing, nothing, nothing, nothing
function exit() error("SCRIPT: exit() is not available") end

-- Remove what 3.2 did not have and what a map script must not reach.
dofile, loadfile, require, module, package, io, os, load, loadstring = nil, nil, nil, nil, nil, nil, nil, nil, nil
