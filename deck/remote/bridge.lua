-- AirPlay Remote bridge.
--
-- Follows shairport-sync's metadata pipe and keeps what is playing in a small
-- JSON file, which the AirPlay Remote widget reads through the Deck's web
-- server (see airplay-remote.cgi).
--
--   lua bridge.lua              follow the live pipe, forever
--   lua bridge.lua FILE OUT     replay a captured metadata FILE into OUT, then exit
--
-- Written for the Deck's Lua 5.1, which has no bit operations and keeps
-- integers in 32 bits: AirPlay's frame counters exceed that, so every number
-- here stays a float and is printed with %.0f or %.2f, never %d.

local PIPE = "/tmp/shairport-sync-metadata"
local STATE = "/tmp/airplay-remote/state.json"
local HUSH = "/usr/share/airplay-remote/hush"
local FRAMES_PER_SEC = 44100
local FRAME_WRAP = 4294967296 -- RTP timestamps are 32-bit and wrap

local B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
local B64_VALUE = {}
for i = 1, #B64 do
	B64_VALUE[B64:sub(i, i)] = i - 1
end

local function base64_decode(text)
	local out, bits, nbits = {}, 0, 0
	for char in text:gmatch("[%w%+/]") do
		bits = bits * 64 + B64_VALUE[char]
		nbits = nbits + 6
		if nbits >= 8 then
			nbits = nbits - 8
			local byte = math.floor(bits / 2 ^ nbits)
			bits = bits - byte * 2 ^ nbits
			out[#out + 1] = string.char(byte)
		end
	end
	return table.concat(out)
end

local function unhex(text)
	return (text:gsub("%x%x", function(pair)
		return string.char(tonumber(pair, 16))
	end))
end

local JSON_ESCAPES = { ['"'] = '\\"', ["\\"] = "\\\\", ["\n"] = "\\n", ["\r"] = "\\r", ["\t"] = "\\t" }

local function json_string(text)
	if text == nil then
		return "null"
	end
	return '"'
		.. (text:gsub('[%c"\\]', function(char)
			return JSON_ESCAPES[char] or string.format("\\u%04x", char:byte())
		end))
		.. '"'
end

local function json_number(value, format)
	if value == nil then
		return "null"
	end
	return string.format(format, value)
end

local state = { session = false, playing = false }

-- Replaying a capture only rebuilds the state; the speaker is left alone.
local live = true

-- Mutes the speaker for the buffered tail of a pause, or unmutes it.
local function hush(on)
	if live then
		os.execute(HUSH .. (on and " on" or " off"))
	end
end

-- Follows a change of play state with the speaker.
local function set_playing(playing)
	if playing ~= state.playing then
		hush(not playing)
	end
	state.playing = playing
end

local function write_state(path)
	local fields = {
		'"session":' .. tostring(state.session),
		'"playing":' .. tostring(state.playing),
		'"title":' .. json_string(state.title),
		'"artist":' .. json_string(state.artist),
		'"album":' .. json_string(state.album),
		'"volume":' .. json_number(state.volume, "%.2f"),
		'"position":' .. json_number(state.position, "%.2f"),
		'"duration":' .. json_number(state.duration, "%.2f"),
		'"progress_at":' .. json_number(state.progress_at, "%.0f"),
		'"dacp_id":' .. json_string(state.dacp_id),
		'"active_remote":' .. json_string(state.active_remote),
		'"updated_at":' .. json_number(os.time(), "%.0f"),
	}
	-- Written beside the target and renamed over it, so a reader never sees half a file.
	local temporary = path .. ".tmp"
	local file = io.open(temporary, "w")
	if not file then
		return
	end
	file:write("{", table.concat(fields, ","), "}\n")
	file:close()
	os.rename(temporary, path)
end

-- Frames between two RTP timestamps, allowing for one wrap of the 32-bit counter.
local function frames_between(from, to)
	local frames = to - from
	if frames < 0 then
		frames = frames + FRAME_WRAP
	end
	return frames
end

local function apply(kind, code, value)
	if kind == "ssnc" and code == "pbeg" then
		state.session, state.playing = true, true
		hush(false)
	elseif kind == "ssnc" and code == "pend" then
		state = { session = false, playing = false }
		hush(false)
	elseif kind == "ssnc" and code == "prsm" then
		set_playing(true)
	elseif kind == "core" and code == "caps" then
		-- The sender's player state, as the iPhone reports it: 1 playing, 2 paused.
		local caps = value:byte(1)
		if caps == 1 then
			set_playing(true)
		elseif caps == 2 then
			set_playing(false)
		end
	elseif kind == "core" and code == "minm" then
		state.title = value
	elseif kind == "core" and code == "asar" then
		state.artist = value
	elseif kind == "core" and code == "asal" then
		state.album = value ~= "" and value or nil
	elseif kind == "ssnc" and code == "pvol" then
		-- "airplay volume,volume dB,lowest dB,highest dB"; AirPlay volume runs -30..0, -144 is mute.
		state.volume = tonumber(value:match("^([^,]+)"))
	elseif kind == "ssnc" and code == "prgr" then
		local start, current, finish = value:match("^(%d+)/(%d+)/(%d+)$")
		if start then
			start, current, finish = tonumber(start), tonumber(current), tonumber(finish)
			state.position = frames_between(start, current) / FRAMES_PER_SEC
			state.duration = frames_between(start, finish) / FRAMES_PER_SEC
			state.progress_at = os.time()
		end
	elseif kind == "ssnc" and code == "daid" then
		state.dacp_id = value
	elseif kind == "ssnc" and code == "acre" then
		state.active_remote = value
	else
		return false
	end
	return true
end

-- One <item>…</item> from the pipe: its type, code and decoded payload.
local function parse_item(item)
	local kind = item:match("<type>(%x+)</type>")
	local code = item:match("<code>(%x+)</code>")
	if not kind or not code then
		return nil
	end
	local data = item:match('<data encoding="base64">(.-)</data>')
	return unhex(kind), unhex(code), data and base64_decode(data) or ""
end

local function follow(input, output)
	local buffer = {}
	local in_bundle = false
	for line in input:lines() do
		buffer[#buffer + 1] = line
		if line:find("</item>", 1, true) then
			local kind, code, value = parse_item(table.concat(buffer, "\n"))
			buffer = {}
			if kind then
				-- Song details arrive as a bundle between "mdst" and "mden"; publish it whole.
				if kind == "ssnc" and code == "mdst" then
					in_bundle = true
				elseif kind == "ssnc" and code == "mden" then
					in_bundle = false
					write_state(output)
				elseif apply(kind, code, value) and not in_bundle then
					write_state(output)
				end
			end
		end
	end
end

local replay, replay_output = arg[1], arg[2]
if replay then
	live = false
	local input = assert(io.open(replay, "r"))
	follow(input, replay_output or STATE)
	input:close()
	return
end

os.execute("mkdir -p /tmp/airplay-remote")
write_state(STATE)
-- Whatever a previous run left behind, start with the speaker on.
hush(false)
while true do
	-- Opening the pipe waits for shairport-sync to start writing; it ends
	-- (and the loop comes round) when shairport-sync restarts.
	local input = io.open(PIPE, "r")
	if input then
		follow(input, STATE)
		input:close()
	end
	state = { session = false, playing = false }
	write_state(STATE)
	hush(false)
	os.execute("sleep 1")
end
