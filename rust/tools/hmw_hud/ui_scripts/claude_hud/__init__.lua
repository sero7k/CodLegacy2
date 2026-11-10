-- Claude HUD for Horizon MW: our game's in-match HUD (crates/ui/src/hud_modern.rs,
-- hud_match.rs) rebuilt in H1's LUI. It replaces H1's HUD widgets:
--   weaponInfoHudDef  -> bottom right: gun name over a dark box with the clip in
--                        big figures and the rest of the ammo, the scorestreak
--                        column over them; bottom left: the player's name over
--                        a thin health bar.
--   teamScoresHudDef  -> top centre: his side's score and bar (blue), the team
--                        badges, the clock, the enemy's score and bar (red),
--                        WINNING / LOSING / TIED under it.
--   obituary          -> bottom left: our kill feed (team colours, him gold).
--   damageFeedbackHudDef -> the hit marker: a white X, red on the kill.
--   minimapHudDef     -> top left: H1's map and tape on our ground, edge and
--                        tape strip (the map itself is engine-drawn).
-- All stay inside H1's own HUD wrappers, so H1 still hides them in menus, the
-- killcam and when the HUD is off. Every call into the game is guarded: if
-- building our widget fails, H1's stock widget is returned instead. H1's
-- remaining boxes (the d-pad, perks, splashes, scoreboard) are restyled by
-- claude_ui's material rules and palette.
--
-- Built into the client as resource CLAUDE_HUD (loaded after CLAUDE_UI).

do
    local ok, mp = pcall(function()
        return Engine.IsMultiplayer()
    end)
    if not ok or not mp then
        return
    end
end

local log = function(msg)
    pcall(function()
        print("[claude_hud] " .. tostring(msg))
    end)
end

-- H1 builds the HUD widgets from modules MPHud loads; load it so their
-- builders exist before we wrap them.
pcall(function()
    luiglobals.require("LUI.mp_hud.MPHud")
end)

-- Our palette (crates/ui/src/shell/theme.rs, hud_modern.rs).
local WHITE = { r = 1, g = 1, b = 1 }
local DIM = { r = 1, g = 1, b = 1 }
local BLUE = { r = 0.26, g = 0.62, b = 0.95 }
local RED = { r = 0.90, g = 0.26, b = 0.22 }
local LOW = { r = 0.95, g = 0.28, b = 0.22 }
local PANEL = { r = 0.04, g = 0.05, b = 0.06 }
local PANEL_ALPHA = 0.62
local TRACK = { r = 0, g = 0, b = 0 }
local TRACK_ALPHA = 0.45
local BAR_BG = { r = 0.35, g = 0.37, b = 0.40 }

-- Our HUD is laid out on a 1080-line screen; LUI's root is 720 high.
local K = 720 / 1080

local function px(v)
    return math.floor(v * K + 0.5)
end

-- A value from the game, or `fallback` when the call is missing or fails.
local function try(fallback, fn, ...)
    if type(fn) ~= "function" then
        return fallback
    end
    local ok, v = pcall(fn, ...)
    if ok and v ~= nil then
        return v
    end
    return fallback
end

local function font(name, fallback)
    local ts = CoD and CoD.TextSettings
    local t = ts and (ts[name] or ts[fallback] or ts.BodyFontBold or ts.TitleFontBold)
    return t and t.Font
end

local white_material = nil
local function white()
    if not white_material then
        white_material = luiglobals.RegisterMaterial("white")
    end
    return white_material
end

-- A solid rectangle.
local function rect(state, color, alpha)
    state.material = white()
    state.color = color
    state.alpha = alpha
    return LUI.UIImage.new(state)
end

-- A text line.
local function text(state, fontname, color, alpha, align)
    state.font = font(fontname, "TitleFontBold")
    state.color = color
    state.alpha = alpha or 1
    if align and LUI.Alignment then
        state.alignment = LUI.Alignment[align]
    end
    return LUI.UIText.new(state)
end

-- Sets an element's state to `state` (registered under a fresh name each time
-- the values change, so colours and widths can follow the game).
local function set_state(el, key, state)
    local sig = ""
    for k, v in pairs(state) do
        if type(v) == "table" then
            sig = sig .. k .. (v.r or 0) .. (v.g or 0) .. (v.b or 0)
        else
            sig = sig .. k .. tostring(v)
        end
    end
    if el["_cl_" .. key] == sig then
        return
    end
    el["_cl_" .. key] = sig
    el:registerAnimationState(key, state)
    el:animateToState(key)
end

-- A full-screen element (our widgets are laid out against the screen).
local function fullscreen()
    return LUI.UIElement.new({
        topAnchor = true,
        bottomAnchor = true,
        leftAnchor = true,
        rightAnchor = true,
        top = 0,
        bottom = 0,
        left = 0,
        right = 0,
    })
end

-- Holds H1's own widget, invisible, so whatever H1 sends it still lands.
local function hide_stock(stock)
    local holder = LUI.UIElement.new({
        topAnchor = true,
        bottomAnchor = true,
        leftAnchor = true,
        rightAnchor = true,
        top = 0,
        bottom = 0,
        left = 0,
        right = 0,
        alpha = 0,
    })
    holder:addElement(stock)
    return holder
end

local function every(el, ms, name, fn)
    el:addElement(LUI.UITimer.new(ms, name))
    el:registerEventHandler(name, function()
        local ok, err = pcall(fn)
        if not ok then
            log(err)
        end
    end)
end

----------------------------------------------------------------------------
-- Bottom right: the gun's name over the ammo box. Bottom left: name, health.
----------------------------------------------------------------------------

local CORNER_RIGHT = px(22)
local CORNER_BOTTOM = px(26)
local BOX_H = px(54)
local BOX_W = px(250)
local NAME_H = px(26)

local function build_weapon_hud()
    local root = fullscreen()

    -- The ammo box.
    local box = rect({
        rightAnchor = true,
        bottomAnchor = true,
        right = -CORNER_RIGHT,
        bottom = -CORNER_BOTTOM,
        width = BOX_W,
        height = BOX_H,
    }, PANEL, PANEL_ALPHA)
    root:addElement(box)

    -- The clip, in big figures: H1's own ammo counter (an owner draw, so it
    -- follows the gun every frame like the stock HUD's).
    local clip = text({
        rightAnchor = true,
        bottomAnchor = true,
        right = -(CORNER_RIGHT + px(92)),
        bottom = -(CORNER_BOTTOM + px(8)),
        width = px(140),
        height = px(40),
    }, "SP_HudAmmoCounterFont", WHITE, 1, "Right")
    local drawn = false
    if clip.setupOwnerdraw and CoD.Ownerdraw and CoD.Ownerdraw.CGAmmoMagazine then
        drawn = pcall(function()
            clip:setupOwnerdraw(CoD.Ownerdraw.CGAmmoMagazine, 1, CoD.TextStyle and CoD.TextStyle.Shadowed)
        end)
    end
    root:addElement(clip)

    -- The rest of the ammo.
    local stock = text({
        rightAnchor = true,
        bottomAnchor = true,
        right = -(CORNER_RIGHT + px(14)),
        bottom = -(CORNER_BOTTOM + px(14)),
        width = px(72),
        height = px(26),
    }, "SP_HudItemAmmoFont", DIM, 0.62, "Left")
    root:addElement(stock)

    -- The gun's name, right-aligned over the box.
    local gun = text({
        rightAnchor = true,
        bottomAnchor = true,
        right = -CORNER_RIGHT,
        bottom = -(CORNER_BOTTOM + BOX_H + px(2)),
        width = px(500),
        height = px(22),
    }, "SP_HudWeaponNameFont", WHITE, 1, "Right")
    root:addElement(gun)

    -- Bottom left: his name over a thin health bar.
    local name = text({
        leftAnchor = true,
        bottomAnchor = true,
        left = px(23),
        bottom = -(px(28) + px(4) + px(4)),
        width = px(400),
        height = px(20),
    }, "TitleFontBold", DIM, 0.62, "Left")
    root:addElement(name)

    local track = rect({
        leftAnchor = true,
        bottomAnchor = true,
        left = px(23),
        bottom = -px(28),
        width = px(210),
        height = math.max(2, px(4)),
    }, TRACK, TRACK_ALPHA)
    root:addElement(track)

    local fill = rect({
        leftAnchor = true,
        bottomAnchor = true,
        left = px(23),
        bottom = -px(28),
        width = px(210),
        height = math.max(2, px(4)),
    }, WHITE, 1)
    root:addElement(fill)

    local function update()
        local weapon = try("", Game.GetPlayerWeaponDisplayName)
        if type(weapon) == "string" and weapon ~= "" then
            gun:setText(Engine.ToUpperCase and try(weapon, Engine.ToUpperCase, weapon) or weapon)
        else
            gun:setText("")
        end

        local hidden = try(false, Game.IsStockAmmoHidden)
        local rest = try(nil, Game.GetPlayerStockAmmo)
        if hidden or rest == nil then
            stock:setText("")
        else
            stock:setText(tostring(rest))
            set_state(stock, "c", {
                rightAnchor = true,
                bottomAnchor = true,
                right = -(CORNER_RIGHT + px(14)),
                bottom = -(CORNER_BOTTOM + px(14)),
                width = px(72),
                height = px(26),
                color = rest == 0 and LOW or DIM,
                alpha = rest == 0 and 1 or 0.62,
            })
        end
        if not drawn then
            clip:setText("")
        end

        local who = try("", Engine.GetDvarString, "name")
        name:setText(who)

        -- His health (H1 gives 0..100; a 0..1 fraction is taken as one).
        local health = try(100, Game.GetPlayerHealth)
        if health <= 1 and health > 0 then
            health = health * 100
        end
        health = math.max(0, math.min(100, health))
        set_state(fill, "h", {
            leftAnchor = true,
            bottomAnchor = true,
            left = px(23),
            bottom = -px(28),
            width = math.floor(px(210) * health / 100 + 0.5),
            height = math.max(2, px(4)),
            color = health <= 35 and RED or WHITE,
            alpha = health > 0 and 1 or 0,
        })
    end

    every(root, 50, "claude_hud_weapon", update)
    root:registerEventHandler("weapon_change", function()
        pcall(update)
    end)
    root:registerEventHandler("playerstate_client_changed", function()
        pcall(update)
    end)
    pcall(update)
    return root
end

----------------------------------------------------------------------------
-- Top centre: the score bar.
----------------------------------------------------------------------------

local SIDE_BAR_W = px(260)
local SIDE_BAR_H = px(12)
local BADGE = px(58)
local CLOCK_W = px(120)
local GAP = px(8)
local TOP = px(15)

local function clock_text(ms)
    local s = math.max(0, math.floor(ms / 1000))
    return string.format("%d:%02d", math.floor(s / 60), s % 60)
end

local function team_name(team, fallback)
    local key = team == "axis" and "g_TeamName_Axis" or "g_TeamName_Allies"
    local raw = try("", Engine.GetDvarString, key)
    if raw == "" then
        return fallback
    end
    local loc = try(raw, Engine.Localize, raw)
    return try(loc, Engine.ToUpperCase, loc)
end

local function build_score_hud()
    local root = fullscreen()
    local half = CLOCK_W / 2 + GAP + BADGE + GAP

    -- One side: name over (score, bar). `dir` -1 is his side (left of the
    -- middle, the bar filling from the middle out), 1 the enemy's.
    local function side(dir, color)
        local s = {}
        local bar_left = dir < 0 and -(half + SIDE_BAR_W) or half
        local base = {
            topAnchor = true,
            left = bar_left,
            top = TOP + BADGE - SIDE_BAR_H,
            width = SIDE_BAR_W,
            height = SIDE_BAR_H,
        }
        local function centred(t)
            t.leftAnchor = false
            t.rightAnchor = false
            return t
        end
        s.bg = rect(centred({
            topAnchor = true,
            left = bar_left,
            top = base.top,
            width = SIDE_BAR_W,
            height = SIDE_BAR_H,
        }), BAR_BG, 0.75)
        s.fill = rect(centred({
            topAnchor = true,
            left = bar_left,
            top = base.top,
            width = 0,
            height = SIDE_BAR_H,
        }), color, 1)
        s.name = text(centred({
            topAnchor = true,
            left = bar_left,
            top = base.top - px(36),
            width = SIDE_BAR_W,
            height = px(22),
        }), "TitleFontBold", WHITE, 1, dir < 0 and "Right" or "Left")
        s.score = text(centred({
            topAnchor = true,
            left = dir < 0 and (bar_left - px(10) - px(80)) or (bar_left + SIDE_BAR_W + px(10)),
            top = base.top - px(6),
            width = px(80),
            height = px(22),
        }), "TitleFontBold", WHITE, 1, dir < 0 and "Right" or "Left")
        root:addElement(s.bg)
        root:addElement(s.fill)
        root:addElement(s.name)
        root:addElement(s.score)
        s.bar_left = bar_left
        s.dir = dir
        s.color = color
        return s
    end

    local function badge(left, team_dvar, color)
        local b = {}
        b.box = rect({
            topAnchor = true,
            leftAnchor = false,
            rightAnchor = false,
            left = left,
            top = TOP,
            width = BADGE,
            height = BADGE,
        }, PANEL, PANEL_ALPHA)
        root:addElement(b.box)
        local icon = try("", Engine.GetDvarString, team_dvar)
        local mat = icon ~= "" and try(nil, luiglobals.RegisterMaterial, icon) or nil
        if mat then
            b.icon = LUI.UIImage.new({
                topAnchor = true,
                leftAnchor = false,
                rightAnchor = false,
                left = left + px(3),
                top = TOP + px(3),
                width = BADGE - px(6),
                height = BADGE - px(6),
                material = mat,
            })
        else
            b.icon = rect({
                topAnchor = true,
                leftAnchor = false,
                rightAnchor = false,
                left = left + (BADGE - px(30)) / 2,
                top = TOP + (BADGE - px(30)) / 2,
                width = px(30),
                height = px(30),
            }, color, 1)
        end
        root:addElement(b.icon)
        return b
    end

    local mine = side(-1, BLUE)
    local theirs = side(1, RED)
    local my_badge, their_badge = nil, nil

    root:addElement(rect({
        topAnchor = true,
        leftAnchor = false,
        rightAnchor = false,
        left = -CLOCK_W / 2,
        top = TOP,
        width = CLOCK_W,
        height = BADGE,
    }, PANEL, PANEL_ALPHA))
    local clock = text({
        topAnchor = true,
        leftAnchor = false,
        rightAnchor = false,
        left = -CLOCK_W / 2,
        top = TOP + (BADGE - px(38)) / 2,
        width = CLOCK_W,
        height = px(38),
    }, "TitleFontBold", WHITE, 1, "Center")
    root:addElement(clock)

    local status = text({
        topAnchor = true,
        leftAnchor = false,
        rightAnchor = false,
        left = -px(200),
        top = TOP + BADGE + px(4),
        width = px(400),
        height = px(20),
    }, "TitleFontBold", DIM, 0.62, "Center")
    root:addElement(status)

    local function fill(s, score, limit)
        local part = 0
        if limit and limit > 0 then
            part = math.max(0, math.min(1, score / limit))
        end
        local w = math.floor(SIDE_BAR_W * part + 0.5)
        set_state(s.fill, "f", {
            topAnchor = true,
            leftAnchor = false,
            rightAnchor = false,
            left = s.dir < 0 and (s.bar_left + SIDE_BAR_W - w) or s.bar_left,
            top = TOP + BADGE - SIDE_BAR_H,
            width = w,
            height = SIDE_BAR_H,
            color = s.color,
            alpha = 1,
        })
    end

    local function update()
        local ffa = try(false, GameX and GameX.gameModeIsFFA)
        local limit = try(0, Engine.GetDvarInt, "ui_scorelimit")
        local yours, enemy = 0, 0
        local my_team = try("allies", Game.GetPlayerTeam)
        if ffa then
            local me = try(nil, Game.GetPlayerClientnum)
            yours = try(0, Game.GetPlayerScore, me)
            local lead = try(nil, Game.GetPlayerScoreInfoAtRank, Teams and Teams.free or "free", 1)
            local leader_name = "LEADER"
            if type(lead) == "table" then
                enemy = lead.score or 0
                if lead.name and lead.name ~= "" then
                    leader_name = try(lead.name, Engine.ToUpperCase, lead.name)
                end
            end
            mine.name:setText("YOU")
            theirs.name:setText(leader_name)
        else
            if my_team ~= "allies" and my_team ~= "axis" then
                my_team = "allies"
            end
            local other = try(my_team == "allies" and "axis" or "allies", Game.GetPlayerOpposingTeam)
            yours = try(0, Game.GetTeamScore, my_team)
            enemy = try(0, Game.GetTeamScore, other)
            mine.name:setText(team_name(my_team, my_team == "allies" and "ALLIES" or "AXIS"))
            theirs.name:setText(team_name(other, other == "allies" and "ALLIES" or "AXIS"))
        end
        mine.score:setText(tostring(yours))
        theirs.score:setText(tostring(enemy))
        fill(mine, yours, limit)
        fill(theirs, enemy, limit)

        -- The badges, once (the team is known by the first update).
        if not my_badge and not ffa then
            local my_dvar = my_team == "axis" and "g_TeamIcon_Axis" or "g_TeamIcon_Allies"
            local their_dvar = my_team == "axis" and "g_TeamIcon_Allies" or "g_TeamIcon_Axis"
            my_badge = badge(-(CLOCK_W / 2 + GAP + BADGE), my_dvar, BLUE)
            their_badge = badge(CLOCK_W / 2 + GAP, their_dvar, RED)
        elseif not my_badge then
            my_badge = badge(-(CLOCK_W / 2 + GAP + BADGE), "", BLUE)
            their_badge = badge(CLOCK_W / 2 + GAP, "", RED)
        end

        -- The clock.
        local left = try(nil, Game.GetTimeLeft)
        if type(left) == "number" and left > 0 then
            clock:setText(clock_text(left))
            set_state(clock, "c", {
                topAnchor = true,
                leftAnchor = false,
                rightAnchor = false,
                left = -CLOCK_W / 2,
                top = TOP + (BADGE - px(38)) / 2,
                width = CLOCK_W,
                height = px(38),
                color = left <= 30000 and LOW or WHITE,
                alpha = 1,
            })
        else
            clock:setText("")
        end

        local word, color, alpha = "TIED", DIM, 0.62
        if yours > enemy then
            word, color, alpha = "WINNING", BLUE, 1
        elseif yours < enemy then
            word, color, alpha = "LOSING", RED, 1
        end
        status:setText(word)
        set_state(status, "s", {
            topAnchor = true,
            leftAnchor = false,
            rightAnchor = false,
            left = -px(200),
            top = TOP + BADGE + px(4),
            width = px(400),
            height = px(20),
            color = color,
            alpha = alpha,
        })
    end

    every(root, 100, "claude_hud_scores", update)
    pcall(update)
    return root
end

----------------------------------------------------------------------------
-- Shared by the pieces below.
----------------------------------------------------------------------------

local GOLD = { r = 0.98, g = 0.72, b = 0.30 }
local HIT_RED = { r = 0.96, g = 0.16, b = 0.12 }
local EDGE = { r = 0.59, g = 0.59, b = 0.59 }

-- A clock in ms for timers (the game's, else our own count of ticks).
local ticks = 0
local function text_width(text, f, h)
    local ok, l, _, r = pcall(luiglobals.GetTextDimensions, text, f, h)
    if ok and type(l) == "number" and type(r) == "number" then
        return math.abs(r - l)
    end
    return string.len(tostring(text)) * h * 0.55
end

local function now_ms(step)
    local t = try(nil, Engine.GetTimeMsecs)
    if type(t) == "number" then
        return t
    end
    ticks = ticks + (step or 0)
    return ticks
end

local function geometry(state)
    local g = {}
    for _, k in ipairs({ "topAnchor", "bottomAnchor", "leftAnchor", "rightAnchor", "top", "bottom", "left", "right", "width", "height" }) do
        if state[k] ~= nil then
            g[k] = state[k]
        end
    end
    return g
end

-- A thin edge round an element (four lines along its sides).
local function edge(parent, color, alpha, w)
    local sides = {
        { topAnchor = true, bottomAnchor = false, leftAnchor = true, rightAnchor = true, top = 0, height = w, left = 0, right = 0 },
        { topAnchor = false, bottomAnchor = true, leftAnchor = true, rightAnchor = true, bottom = 0, height = w, left = 0, right = 0 },
        { topAnchor = true, bottomAnchor = true, leftAnchor = true, rightAnchor = false, top = 0, bottom = 0, left = 0, width = w },
        { topAnchor = true, bottomAnchor = true, leftAnchor = false, rightAnchor = true, top = 0, bottom = 0, right = 0, width = w },
    }
    local lines = {}
    for _, s in ipairs(sides) do
        local line = rect(s, color, alpha)
        parent:addElement(line)
        table.insert(lines, { el = line, state = s })
    end
    return lines
end

local function recolor_edge(lines, key, color, alpha)
    for _, l in ipairs(lines) do
        local s = {}
        for k, v in pairs(l.state) do
            s[k] = v
        end
        s.color = color
        s.alpha = alpha
        set_state(l.el, key, s)
    end
end

----------------------------------------------------------------------------
-- Bottom right, over the gun's name: his scorestreaks in a column of
-- squares on the right edge (hud_modern.rs, as Modern Warfare 3's), the
-- dearest at the top; the earned one lit in the accent. From H1's own
-- scorestreak omnvars (ks_icon_1..3, ks_hasStreak); the column stays empty
-- when the match sends none.
----------------------------------------------------------------------------

local STREAK = px(56)
local STREAK_GAP = math.max(2, px(3))

local function streak_material(idx)
    local tbl = KillstreakTable
    if type(tbl) ~= "table" or not tbl.Cols then
        return nil
    end
    local name = try("", Engine.TableLookup, tbl.File, tbl.Cols.Index, tostring(idx), tbl.Cols.EarnedIcon)
    if type(name) ~= "string" or name == "" or name == "specialty_ks_null" then
        return nil
    end
    return try(nil, luiglobals.RegisterMaterial, name)
end

-- Whether his streak `i` is earned (H1 keeps a bit per streak; bit 0 is
-- the care package's "gimme" slot).
local function has_streak(i)
    local bits = try(0, Game.GetOmnvar, "ks_hasStreak")
    if type(bits) ~= "number" then
        return false
    end
    return math.floor(bits / (2 ^ i)) % 2 == 1
end

local function add_streak_column(root)
    local base = CORNER_BOTTOM + BOX_H + px(2) + NAME_H + px(4)
    local squares = {}
    for i = 1, 3 do
        local bottom = -(base + (i - 1) * (STREAK + STREAK_GAP))
        local g = {
            rightAnchor = true,
            bottomAnchor = true,
            leftAnchor = false,
            topAnchor = false,
            right = -CORNER_RIGHT,
            bottom = bottom,
            width = STREAK,
            height = STREAK,
        }
        local box = LUI.UIElement.new(g)
        local ground = rect({ topAnchor = true, bottomAnchor = true, leftAnchor = true, rightAnchor = true, top = 0, bottom = 0, left = 0, right = 0 }, PANEL, PANEL_ALPHA)
        box:addElement(ground)
        local glow = rect({ topAnchor = true, bottomAnchor = true, leftAnchor = true, rightAnchor = true, top = 0, bottom = 0, left = 0, right = 0 }, BLUE, 0)
        box:addElement(glow)
        local inner = math.floor(STREAK * 0.74 + 0.5)
        local icon = LUI.UIImage.new({
            topAnchor = false,
            bottomAnchor = false,
            leftAnchor = false,
            rightAnchor = false,
            top = -inner / 2,
            bottom = inner / 2,
            left = -inner / 2,
            right = inner / 2,
            material = white(),
            alpha = 0,
        })
        box:addElement(icon)
        local lines = edge(box, WHITE, 0.16, 1)
        local shown, hidden = {}, {}
        for k, v in pairs(g) do
            shown[k] = v
            hidden[k] = v
        end
        shown.alpha = 1
        hidden.alpha = 0
        box:registerAnimationState("shown", shown)
        box:registerAnimationState("hidden", hidden)
        box:animateToState("hidden")
        root:addElement(box)
        squares[i] = { box = box, glow = glow, icon = icon, lines = lines, idx = nil, inner = inner }
    end

    local function update()
        for i = 1, 3 do
            local s = squares[i]
            local idx = try(0, Game.GetOmnvar, "ks_icon_" .. i)
            if type(idx) ~= "number" or idx <= 0 then
                if s.idx ~= 0 then
                    s.idx = 0
                    s.box:animateToState("hidden")
                end
            else
                if s.idx ~= idx then
                    s.idx = idx
                    local mat = streak_material(idx)
                    if mat then
                        pcall(function()
                            s.icon:setImage(mat)
                        end)
                    end
                    s.has_icon = mat ~= nil
                    s.box:animateToState("shown")
                end
                local ready = has_streak(i)
                local half = s.inner / 2
                set_state(s.icon, "i", {
                    topAnchor = false,
                    bottomAnchor = false,
                    leftAnchor = false,
                    rightAnchor = false,
                    top = -half,
                    bottom = half,
                    left = -half,
                    right = half,
                    color = WHITE,
                    alpha = s.has_icon and (ready and 1 or 0.42) or 0,
                })
                set_state(s.glow, "g", {
                    topAnchor = true,
                    bottomAnchor = true,
                    leftAnchor = true,
                    rightAnchor = true,
                    top = 0,
                    bottom = 0,
                    left = 0,
                    right = 0,
                    color = BLUE,
                    alpha = ready and 0.3 or 0,
                })
                recolor_edge(s.lines, "e", ready and BLUE or WHITE, ready and 1 or 0.16)
            end
        end
    end
    every(root, 250, "claude_hud_streaks", update)
    pcall(update)
end

----------------------------------------------------------------------------
-- Bottom left, over his name: the kill feed (hud_match.rs): "Attacker
-- [weapon] Victim" rows on a dark strip, names in team colours (his side
-- blue, the enemy red, him gold), at most five, each going after six
-- seconds. Fed by H1's own obituary events.
----------------------------------------------------------------------------

local FEED_ROWS = 5
local FEED_LIFE = 6000
local FEED_OUT = 600
local FEED_TEXT = px(22)
local FEED_ROW_H = FEED_TEXT + px(6)
local FEED_GAP = math.max(1, px(3))
local FEED_BOTTOM = px(72)
local FEED_LEFT = px(23)

-- H1's obituary module (its getColorForClientNum tells his side, the
-- enemy's and his own apart).
local obituary_module = nil
local function find_obituary()
    if obituary_module == nil then
        obituary_module = false
        pcall(function()
            for _, v in pairs(LUI.mp_hud) do
                if type(v) == "table" and type(rawget(v, "getColorForClientNum")) == "function" then
                    obituary_module = v
                end
            end
        end)
    end
    return obituary_module or nil
end

local function name_color(cn)
    if type(cn) ~= "number" or cn < 0 then
        return WHITE
    end
    local me = try(nil, Game.GetPlayerClientnum)
    if me ~= nil and cn == me then
        return GOLD
    end
    local hud = Swatches and Swatches.HUD
    local m = find_obituary()
    if m and hud then
        local team = try(nil, Game.GetPlayerTeam)
        local ok, c = pcall(m.getColorForClientNum, cn, team)
        if ok and c ~= nil then
            if c == hud.Me then
                return GOLD
            elseif c == hud.Enemies then
                return RED
            elseif c == hud.Allies then
                return BLUE
            end
        end
    end
    if try(false, GameX and GameX.gameModeIsFFA) then
        return RED
    end
    return WHITE
end

local function player_name(cn)
    if type(cn) ~= "number" or cn < 0 then
        return ""
    end
    local n = try("", Game.GetPlayerName, cn)
    return type(n) == "string" and n or ""
end

local function build_feed()
    local root = fullscreen()
    local rows = {}
    local feed_font = font("TitleFontTinyBold", "TitleFontBold")

    local function place(row, n, alpha, ms)
        local state = {
            leftAnchor = true,
            bottomAnchor = true,
            topAnchor = false,
            rightAnchor = false,
            left = FEED_LEFT,
            bottom = -(FEED_BOTTOM + n * (FEED_ROW_H + FEED_GAP)),
            width = row.width,
            height = FEED_ROW_H,
            alpha = alpha,
        }
        row.key = (row.key or 0) + 1
        row.el:registerAnimationState("p" .. row.key, state)
        row.el:animateToState("p" .. row.key, ms or 0)
    end

    local function layout()
        local n = #rows
        for i, row in ipairs(rows) do
            if not row.leaving then
                place(row, n - i, 1, 0)
            end
        end
    end

    local function add_row(parts)
        local pad = px(8)
        local gap = px(7)
        local x = pad
        local el = LUI.UIElement.new({
            leftAnchor = true,
            bottomAnchor = true,
            left = FEED_LEFT,
            bottom = -FEED_BOTTOM,
            width = 10,
            height = FEED_ROW_H,
            alpha = 0,
        })
        local strip = rect({ topAnchor = true, bottomAnchor = true, leftAnchor = true, rightAnchor = true, top = 0, bottom = 0, left = 0, right = 0 }, PANEL, 0.42)
        el:addElement(strip)
        for _, p in ipairs(parts) do
            if p.text and p.text ~= "" then
                local w = math.ceil(text_width(p.text, feed_font, FEED_TEXT))
                local t = LUI.UIText.new({
                    leftAnchor = true,
                    topAnchor = false,
                    bottomAnchor = false,
                    rightAnchor = false,
                    left = x,
                    top = -FEED_TEXT / 2,
                    bottom = FEED_TEXT / 2,
                    width = w + 2,
                    font = feed_font,
                    color = p.color,
                    alpha = 1,
                })
                t:setText(p.text)
                pcall(function()
                    t:setTextStyle(CoD.TextStyle.Shadowed)
                end)
                el:addElement(t)
                x = x + w + gap
            elseif p.material then
                local h = math.floor(FEED_TEXT * 1.05 + 0.5)
                local wide = p.aspect or 1
                local w, ih = h * wide * 1.1, h
                if wide >= 4 then
                    w, ih = h * 3.2, h * 0.8
                end
                w = math.floor(w + 0.5)
                el:addElement(LUI.UIImage.new({
                    leftAnchor = true,
                    topAnchor = false,
                    bottomAnchor = false,
                    rightAnchor = false,
                    left = x,
                    top = -ih / 2,
                    bottom = ih / 2,
                    width = w,
                    material = p.material,
                    alpha = 1,
                }))
                x = x + w + gap
            end
        end
        local row = { el = el, width = x - gap + px(12), born = now_ms(0) }
        root:addElement(el)
        table.insert(rows, row)
        while #rows > FEED_ROWS do
            local old = table.remove(rows, 1)
            pcall(function()
                old.el:close()
            end)
        end
        layout()
    end

    root:registerEventHandler("obituary", function(_, event)
        local ok, err = pcall(function()
            if type(event) ~= "table" then
                return
            end
            local parts = {}
            local attacker, victim = event.attacker, event.victim
            if attacker ~= nil and attacker ~= victim then
                local a = player_name(attacker)
                if a ~= "" then
                    table.insert(parts, { text = a, color = name_color(attacker) })
                end
            end
            if event.icon ~= nil then
                local aspect = try(1, Engine.GetMaterialAspectRatio, event.icon)
                table.insert(parts, { material = event.icon, aspect = type(aspect) == "number" and aspect or 1 })
            else
                table.insert(parts, { text = "-", color = DIM })
            end
            local v = player_name(victim)
            if v ~= "" then
                table.insert(parts, { text = v, color = name_color(victim) })
            end
            add_row(parts)
        end)
        if not ok then
            log("feed: " .. tostring(err))
        end
    end)

    root:registerEventHandler("game_message", function(_, event)
        pcall(function()
            if type(event) == "table" and type(event.message) == "string" and event.message ~= "" then
                add_row({ { text = event.message, color = WHITE } })
            end
        end)
    end)

    every(root, 100, "claude_hud_feed", function()
        local t = now_ms(100)
        local i = 1
        local changed = false
        while i <= #rows do
            local row = rows[i]
            local age = t - row.born
            if age >= FEED_LIFE + FEED_OUT then
                pcall(function()
                    row.el:close()
                end)
                table.remove(rows, i)
                changed = true
            else
                if age >= FEED_LIFE and not row.leaving then
                    row.leaving = true
                    place(row, #rows - i, 0, FEED_OUT)
                end
                i = i + 1
            end
        end
        if changed then
            layout()
        end
    end)
    return root
end

----------------------------------------------------------------------------
-- The middle of the screen: the hit marker (hud_match.rs), a white X on
-- each hit that pops in a little bigger and fades in ~0.3 s; red and a
-- little bigger on the killing shot. H1's own damage_feedback omnvar drives
-- it; the X is H1's own picture of one.
----------------------------------------------------------------------------

local HIT_SIDE = px(30)

local function build_hitmarker()
    local root = fullscreen()
    local mat = try(nil, luiglobals.RegisterMaterial, "damage_feedback")
    if not mat then
        error("no damage_feedback material")
    end
    local function st(side, color, alpha)
        local h = math.floor(side / 2 + 0.5)
        return {
            topAnchor = false,
            bottomAnchor = false,
            leftAnchor = false,
            rightAnchor = false,
            top = -h,
            bottom = h,
            left = -h,
            right = h,
            color = color,
            alpha = alpha,
        }
    end
    local first = st(HIT_SIDE, WHITE, 0)
    first.material = mat
    local x = LUI.UIImage.new(first)
    x:registerAnimationState("pop_hit", st(HIT_SIDE * 1.38, WHITE, 1))
    x:registerAnimationState("hit", st(HIT_SIDE, WHITE, 1))
    x:registerAnimationState("pop_kill", st(HIT_SIDE * 1.5, HIT_RED, 1))
    x:registerAnimationState("kill", st(HIT_SIDE * 1.12, HIT_RED, 1))
    x:registerAnimationState("off_hit", st(HIT_SIDE, WHITE, 0))
    x:registerAnimationState("off_kill", st(HIT_SIDE * 1.12, HIT_RED, 0))
    root:addElement(x)

    local shown_at, kind = nil, nil
    root:registerOmnvarHandler("damage_feedback", function(_, event)
        local ok, err = pcall(function()
            local v = type(event) == "table" and event.value or nil
            if v == nil then
                v = try(nil, Game.GetOmnvar, "damage_feedback")
            end
            if v == nil or v == "" or v == 0 then
                return
            end
            local s = tostring(v)
            local kill = string.find(s, "killshot", 1, true) ~= nil
            -- (A hit just after the kill's X keeps it red.)
            if not kill and kind == "kill" and shown_at and now_ms(0) - shown_at < 120 then
                return
            end
            kind = kill and "kill" or "hit"
            shown_at = now_ms(0)
            x:animateToState("pop_" .. kind, 0)
            x:animateToState(kind, 70)
        end)
        if not ok then
            log("hit marker: " .. tostring(err))
        end
    end)
    every(root, 30, "claude_hud_hit", function()
        if shown_at then
            local life = kind == "kill" and 420 or 300
            local age = now_ms(30) - shown_at
            if age >= 80 then
                x:animateToState("off_" .. kind, math.max(60, life - 80))
                shown_at = nil
            end
        end
    end)
    return root
end

----------------------------------------------------------------------------
-- Top left: the minimap (hud_modern.rs), square-cornered on our dark
-- ground with a thin edge, the compass tape on a dark strip along its top.
-- The map, its marks and the tape are still H1's own (the engine draws
-- them); only what is round them changes. H1's backing becomes our ground
-- (claude_ui's material rules).
----------------------------------------------------------------------------

local function wrap_minimap()
    local builders = LUI.MenuBuilder and LUI.MenuBuilder.m_types_build
    local stock = builders and builders.minimapHudDef
    if not stock then
        log("no minimapHudDef: left as it is")
        return
    end
    builders.minimapHudDef = function(...)
        local mb = LUI.MenuBuilder
        local orig_add = mb.BuildAddChild
        mb.BuildAddChild = function(parent, def, ...)
            pcall(function()
                if type(def) == "table" and def.type == "UICompass" and def.states and def.states.default then
                    -- The tape's strip, under the tape.
                    local g = geometry(def.states.default)
                    g.material = white()
                    g.color = PANEL
                    g.alpha = 0.82
                    parent:addElement(LUI.UIImage.new(g))
                end
            end)
            return orig_add(parent, def, ...)
        end
        local ok, root = pcall(stock, ...)
        mb.BuildAddChild = orig_add
        if not ok then
            error(root)
        end
        pcall(function()
            local map = root:getFirstDescendentById("minimap")
            if map then
                edge(map, EDGE, 0.78, 1)
            end
            local tape = root:getFirstDescendentById("minimapCompass")
            if tape then
                -- The accent under the tape.
                tape:addElement(rect({ topAnchor = false, bottomAnchor = true, leftAnchor = true, rightAnchor = true, bottom = 0, height = 1, left = 0, right = 0 }, BLUE, 0.7))
            end
        end)
        return root
    end
end

----------------------------------------------------------------------------
-- Wrapping H1's widget builders.
----------------------------------------------------------------------------

local function wrap(type_name, build_ours)
    local builders = LUI.MenuBuilder and LUI.MenuBuilder.m_types_build
    local stock_builder = builders and builders[type_name]
    if not stock_builder then
        log("no " .. type_name .. ": left as it is")
        return
    end
    builders[type_name] = function(...)
        local stock = stock_builder(...)
        local ok, ours = pcall(build_ours)
        if not ok or not ours then
            log(type_name .. ": " .. tostring(ours) .. " (stock HUD kept)")
            return stock
        end
        local ok2, holder = pcall(hide_stock, stock)
        if not ok2 then
            log(type_name .. ": " .. tostring(holder) .. " (stock HUD kept)")
            return stock
        end
        ours:addElement(holder)
        return ours
    end
end

wrap("weaponInfoHudDef", function()
    local root = build_weapon_hud()
    local ok, err = pcall(add_streak_column, root)
    if not ok then
        log("streaks: " .. tostring(err))
    end
    return root
end)
wrap("teamScoresHudDef", build_score_hud)
wrap("obituary", build_feed)
wrap("damageFeedbackHudDef", build_hitmarker)
pcall(wrap_minimap)
log("loaded")
