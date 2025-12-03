-- @macro
local v = 1

function foo(v)
    return v + 1
end

do
    local v = 10
    assert(foo(v) == 11)
end

assert(v == 1)

local function bar(v)
    return foo(v)
end

assert(bar(5) == 6)

function animation_db:load()
	local function load_ani_file(f)
		local ok, achunk = pcall(FS.load, f)

		if not ok then
			assert(false, string.format("Failed to load animation file %s.\n%s", f, achunk))
		end

		local ok, atable = pcall(achunk)

		if not ok then
			assert(false, string.format("Failed to eval animation chunk for file:%s", f, atable))
		end

		if not atable then
			assert(false, string.format("Failed to load animation file %s. Could not find .animations", f))
		end

		if atable.animations then
			atable = atable.animations
		end

		for k, v in pairs(atable) do
			if self.db[k] then
				log.error("Animation %s already exists. Not loading it from file %s", k, f)
				-- assert(false, string.format("Animation %s already exists. Not loading it from file %s", k, f))
			else
				self.db[k] = v
			end
		end
	end

	self.db = {}

	local f = string.format("%s/data/game_animations.lua", KR_PATH_GAME)
end