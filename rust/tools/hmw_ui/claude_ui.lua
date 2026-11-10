-- Claude UI for Horizon MW: our front end's look (crates/ui/src/shell: theme.rs,
-- view.rs) laid over H1's own menus, so every menu keeps all it does (server
-- browser, private match, mods, Create-a-Class, options, the pause menu) and
-- only its look changes:
--
--   * H1's green accent becomes our blue: every colour in Colors / Swatches /
--     HudStandards that is green is turned to the same lightness of our blue
--     (theme.rs ACCENT 0.26, 0.62, 0.95). Friendly is blue, the enemy red.
--   * The menu rows (H1ButtonBackground): a dark translucent band with a
--     white bar at its left; the row in focus a band in the accent with a
--     lighter bar and a point at its end (view.rs menu_rows). H1's scanlines,
--     glows, strokes and contours are left out.
--   * Every menu (LUI.MenuTemplate): a deeper shade down its left so the rows
--     read over the scene, and Infinite Warfare's lit band over the footer.
--   * The main menu: our CALL OF DUTY logo in place of Modern Warfare
--     Remastered's.
--   * In a match: H1's HUD boxes (their blurred backings and borders) become
--     our flat navy panels; the scoreboard's headers, bars and highlight are
--     in our colours.
--
-- Everything is guarded: a hook that fails leaves H1's own piece as it was.
-- Built into the client (resource CLAUDE_UI), loaded after the game's own
-- LUI scripts and before claude_hud. Source copy: tools/hmw_ui in the
-- bo2-multiplayer-rebuild repo.

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
        print("[claude_ui] " .. tostring(msg))
    end)
end

local in_frontend = false
pcall(function()
    in_frontend = Engine.InFrontend() and true or false
end)

----------------------------------------------------------------------------
-- Our palette (theme.rs), shared with claude_hud through ClaudeUI.
----------------------------------------------------------------------------

local UI = {}
ClaudeUI = UI

UI.ACCENT = { r = 0.26, g = 0.62, b = 0.95 }
UI.ACCENT_LIFT = { r = 0.36, g = 0.72, b = 1.0 }
UI.ENEMY = { r = 0.90, g = 0.26, b = 0.22 }
UI.WHITE = { r = 1, g = 1, b = 1 }
UI.TEXT = { r = 0.92, g = 0.94, b = 0.97 }
UI.DIM = { r = 0.80, g = 0.84, b = 0.90 }
UI.GOLD = { r = 0.98, g = 0.72, b = 0.30 }
UI.GREEN = { r = 0.36, g = 0.86, b = 0.48 }
UI.PANEL = { r = 0.025, g = 0.04, b = 0.07 }
UI.BLACK = { r = 0, g = 0, b = 0 }
UI.SHADE = { r = 0.0, g = 0.01, b = 0.03 }

-- Our screens are laid out on 1080 lines; LUI's root is 720 high.
UI.K = 720 / 1080
function UI.px(v)
    return math.floor(v * UI.K + 0.5)
end

local orig_register = luiglobals.RegisterMaterial
UI.orig_register = orig_register

local material_cache = {}
function UI.material(name)
    local m = material_cache[name]
    if m == nil then
        local ok, got = pcall(orig_register, name)
        m = ok and got or false
        material_cache[name] = m
    end
    return m or nil
end

-- Whether the game has a material (a missing one draws the default
-- checkerboard).
local have_cache = {}
function UI.has_material(name)
    local h = have_cache[name]
    if h == nil then
        h = false
        local ok, missing = pcall(function()
            return game:isdefaultmaterial(name)
        end)
        if ok and missing == false then
            h = true
        end
        have_cache[name] = h
    end
    return h
end

function UI.font(name, fallback)
    local ts = CoD and CoD.TextSettings
    local t = ts and (ts[name] or ts[fallback] or ts.TitleFontBold or ts.BodyFontBold)
    return t and t.Font
end

function UI.font_height(name, fallback)
    local ts = CoD and CoD.TextSettings
    local t = ts and (ts[name] or ts[fallback])
    return t and t.Height
end

function UI.text_width(text, font, height)
    local ok, l, t, r = pcall(GetTextDimensions, text, font, height)
    if ok and type(l) == "number" and type(r) == "number" then
        return math.abs(r - l)
    end
    return string.len(tostring(text)) * height * 0.5
end

function UI.copy(t)
    local c = {}
    for k, v in pairs(t) do
        c[k] = v
    end
    return c
end

----------------------------------------------------------------------------
-- 1. H1's green accent becomes our blue.
----------------------------------------------------------------------------

local function greenish(c)
    return c.g > c.r + 0.06 and c.g >= c.b + 0.02
end

-- The same colour's lightness in our blue (its brightest channel and its
-- darkest kept; the hue ours).
local function blueify(c)
    local mx = math.max(c.r, c.g, c.b)
    local mn = math.min(c.r, c.g, c.b)
    local span = mx - mn
    c.r = mn + span * 0.274
    c.g = mn + span * 0.653
    c.b = mx
end

local function recolor(t, depth, seen)
    for k, v in pairs(t) do
        if type(v) == "table" and not seen[v] and k ~= "green" then
            seen[v] = true
            if type(v.r) == "number" and type(v.g) == "number" and type(v.b) == "number" then
                if greenish(v) then
                    pcall(blueify, v)
                end
            elseif depth < 3 then
                recolor(v, depth + 1, seen)
            end
        end
    end
end

do
    local seen = {}
    for _, name in ipairs({ "Colors", "Swatches" }) do
        local t = luiglobals[name]
        if type(t) == "table" then
            local ok, err = pcall(recolor, t, 0, seen)
            if not ok then
                log(name .. ": " .. tostring(err))
            end
        end
    end
    if CoD and type(CoD.HudStandards) == "table" then
        pcall(recolor, CoD.HudStandards, 0, seen)
    end
end

-- Button text: white on the band in focus, our text colour otherwise.
do
    local function set_text_colors(t, depth, seen)
        if seen[t] then
            return
        end
        seen[t] = true
        if t.text_focus_color ~= nil then
            pcall(function()
                t.text_focus_color = UI.WHITE
            end)
        end
        if t.text_default_color ~= nil then
            pcall(function()
                t.text_default_color = UI.TEXT
            end)
        end
        if depth < 3 then
            for _, v in pairs(t) do
                if type(v) == "table" and type(v.r) ~= "number" then
                    set_text_colors(v, depth + 1, seen)
                end
            end
        end
    end
    local seen = {}
    for _, gbs in ipairs({ CoD and CoD.GenericButtonSettings, LUI and LUI.GenericButtonSettings }) do
        if type(gbs) == "table" then
            pcall(set_text_colors, gbs, 0, seen)
        end
    end
end

----------------------------------------------------------------------------
-- 2. H1's materials drawn in our look.
--
-- RegisterMaterial is wrapped to remember each material's name; images
-- (their first state and every animation state that names a material) whose
-- material is one of these are drawn in a flat colour instead, or left out.
----------------------------------------------------------------------------

local names = {}
luiglobals.RegisterMaterial = function(name, ...)
    local m = orig_register(name, ...)
    if m ~= nil and type(name) == "string" then
        pcall(function()
            names[m] = name
        end)
    end
    return m
end

-- name -> { color, alpha (multiplied), hide, ingame, logo }
local RULES = {}
local function rule(list, r)
    for _, n in ipairs(list) do
        RULES[n] = r
    end
end

-- The menu rows.
rule({ "h1_ui_btn_unfocused", "h1_btn_unfocused" }, { color = UI.BLACK, alpha = 0.42 })
rule({ "h1_btn_locked_unfocused" }, { color = UI.BLACK, alpha = 0.28 })
rule({ "h1_ui_btn_focused_fill", "h1_btn_reg_selected", "h1_btn_reg_current_selected", "h1_btn_option_popup_btn_selected" },
    { color = UI.ACCENT, alpha = 0.95 })
rule({
    "h1_ui_btn_focused_scanlines", "h1_ui_btn_focused_stroke", "h1_ui_btn_focused_stroke_square",
    "h1_ui_btn_focused_inner_glow", "h1_ui_btn_focused_glow", "h1_btn_focused_overlay_glow",
    "h1_btn_reg_selected_contour", "h1_btn_reg_selected_greenline", "h1_btn_reg_selected_glow",
    "h1_btn_focused_stroke", "h1_btn_focused_scanlines", "h1_btn_focused_rect_stroke", "h1_btn_locked_overlay",
    "h1_ui_header_glitch", "h1_ui_footer_glitch",
}, { hide = true })

-- The main menu's logo: ours over it.
rule({ "h1_logo_mw_medium" }, { logo = true })

-- The HUD's boxes: our flat navy panels.
rule({
    "h1_hud_box_blur", "h1_hud_weapwidget_blur", "h1_hud_dpad_blur", "h1_hud_minimap_backing",
    "h1_ks_splash_backing", "h1_ui_scoreboard_teamheader_bg", "h1_ui_scoreboard_dataheader_bg",
}, { color = UI.PANEL, alpha = 0.72, ingame = true })
rule({
    "h1_hud_box_border", "h1_hud_weapwidget_border", "h1_hud_dpad_border", "h1_hud_dpad_extra",
    "h1_ui_hud_deco_angledbar_anchor", "h1_ui_dot_pattern", "h1_ui_dot_pattern_lg",
}, { hide = true, ingame = true })
rule({ "h1_ui_scoreboard_colorbar_team", "h1_ui_scoreboard_highlight_frame", "scorestreak_fill", "h1_hud_splash_glow" },
    { color = UI.ACCENT, ingame = true })
rule({ "h1_ui_scoreboard_colorbar_enemy" }, { color = UI.ENEMY, ingame = true })

UI.RULES = RULES

local function rule_for(material)
    if material == nil then
        return nil
    end
    local n = names[material]
    local r = n and RULES[n]
    if r and r.ingame and in_frontend then
        return nil
    end
    return r
end

local can_clear = nil
local function clear_material()
    if can_clear == nil then
        can_clear = UI.has_material("ui_transparent")
    end
    return can_clear and UI.material("ui_transparent") or nil
end

-- A state drawn our way (a copy; H1's own tables are left alone).
local function restyle(state, r)
    local s = UI.copy(state)
    if r.hide or r.logo then
        local clear = clear_material()
        if clear then
            s.material = clear
        else
            s.material = UI.material("white")
            s.alpha = 0
        end
    else
        s.material = UI.material("white")
        s.color = r.color
        if r.alpha then
            s.alpha = (type(state.alpha) == "number" and state.alpha or 1) * r.alpha
        end
    end
    return s
end

local orig_ras = LUI.UIElement.registerAnimationState
local function our_ras(self, name, state, ...)
    if type(state) == "table" and state.material ~= nil then
        local r = rule_for(state.material)
        if r and not r.logo then
            local ok, s = pcall(restyle, state, r)
            if ok then
                state = s
            end
        end
    end
    return orig_ras(self, name, state, ...)
end

if type(orig_ras) == "function" then
    LUI.UIElement.registerAnimationState = our_ras
    -- (Classes that keep their own copy of it.)
    for _, class in pairs(LUI) do
        if type(class) == "table" and class ~= LUI.UIElement then
            local own = nil
            pcall(function()
                own = rawget(class, "registerAnimationState")
            end)
            if own == orig_ras then
                pcall(function()
                    class.registerAnimationState = our_ras
                end)
            end
        end
    end
end

-- Our CALL OF DUTY logo, filling the box H1's logo had.
local function add_logo(holder)
    local font = UI.font("TitleFontLarge", "H1TitleFont")
    local h = 64
    local text = LUI.UIText.new({
        topAnchor = false,
        bottomAnchor = false,
        leftAnchor = true,
        rightAnchor = true,
        left = 0,
        right = 0,
        top = -h / 2 - 8,
        bottom = h / 2 - 8,
        font = font,
        alignment = LUI.Alignment and LUI.Alignment.Center,
        color = { r = 0.88, g = 0.895, b = 0.92 },
        alpha = 1,
    })
    holder:addElement(text)
    if text.setText then
        text:setText("CALL OF DUTY")
        pcall(function()
            text:setTextStyle(CoD.TextStyle.Shadowed)
        end)
    end
    -- The accent rule under it.
    holder:addElement(LUI.UIImage.new({
        topAnchor = false,
        bottomAnchor = false,
        leftAnchor = false,
        rightAnchor = false,
        left = -150,
        right = 150,
        top = h / 2 - 2,
        bottom = h / 2 + 1,
        material = UI.material("white"),
        color = UI.ACCENT,
        alpha = 0.9,
    }))
end

local orig_image_new = LUI.UIImage.new
LUI.UIImage.new = function(state, ...)
    local r = nil
    if type(state) == "table" then
        r = rule_for(state.material)
    end
    if not r then
        return orig_image_new(state, ...)
    end
    if r.logo and not clear_material() then
        -- (No clear material to draw it with: H1's logo stays.)
        return orig_image_new(state, ...)
    end
    local ok, s = pcall(restyle, state, r)
    if not ok then
        return orig_image_new(state, ...)
    end
    local el = orig_image_new(s, ...)
    if r.logo then
        pcall(add_logo, el)
    elseif r.hide and not clear_material() then
        -- Kept out of sight whatever state it is sent to.
        pcall(function()
            el.registerAnimationState = function(self, name, st, ...)
                if type(st) == "table" then
                    st = UI.copy(st)
                    st.alpha = 0
                end
                return our_ras(self, name, st, ...)
            end
        end)
    end
    return el
end

----------------------------------------------------------------------------
-- 3. The rows: a white bar at the left (the accent's lighter blue on the
-- row in focus) and a point at the focused row's end.
----------------------------------------------------------------------------

local function row_marks(bg)
    local bar_w = math.max(3, UI.px(6))
    local bar = LUI.UIImage.new({
        topAnchor = true,
        bottomAnchor = true,
        leftAnchor = true,
        rightAnchor = false,
        top = 0,
        bottom = 0,
        left = 0,
        width = bar_w,
        material = UI.material("white"),
        color = UI.WHITE,
        alpha = 0.88,
    })
    bar:registerAnimationState("claude_over", {
        topAnchor = true,
        bottomAnchor = true,
        leftAnchor = true,
        rightAnchor = false,
        top = 0,
        bottom = 0,
        left = 0,
        width = bar_w,
        color = UI.ACCENT_LIFT,
        alpha = 1,
    })
    bar:registerAnimationState("claude_up", {
        topAnchor = true,
        bottomAnchor = true,
        leftAnchor = true,
        rightAnchor = false,
        top = 0,
        bottom = 0,
        left = 0,
        width = bar_w,
        color = UI.WHITE,
        alpha = 0.88,
    })
    bar:registerAnimationState("claude_locked", {
        topAnchor = true,
        bottomAnchor = true,
        leftAnchor = true,
        rightAnchor = false,
        top = 0,
        bottom = 0,
        left = 0,
        width = bar_w,
        color = UI.WHITE,
        alpha = 0.35,
    })

    -- The point: a square turned on its corner, half out past the end.
    local s = 18
    local point = LUI.UIImage.new({
        topAnchor = false,
        bottomAnchor = false,
        leftAnchor = false,
        rightAnchor = true,
        top = -s / 2,
        bottom = s / 2,
        right = s / 2,
        width = s,
        zRot = 45,
        material = UI.material("white"),
        color = UI.ACCENT,
        alpha = 0,
    })
    point:registerAnimationState("claude_over", {
        topAnchor = false,
        bottomAnchor = false,
        leftAnchor = false,
        rightAnchor = true,
        top = -s / 2,
        bottom = s / 2,
        right = s / 2,
        width = s,
        zRot = 45,
        color = UI.ACCENT,
        alpha = 0.95,
    })
    point:registerAnimationState("claude_up", {
        topAnchor = false,
        bottomAnchor = false,
        leftAnchor = false,
        rightAnchor = true,
        top = -s / 2,
        bottom = s / 2,
        right = s / 2,
        width = s,
        zRot = 45,
        color = UI.ACCENT,
        alpha = 0,
    })

    local function on(state, ms)
        return function(el)
            pcall(function()
                el:animateToState(state, ms)
            end)
        end
    end
    for _, el in ipairs({ bar, point }) do
        el:registerEventHandler("button_over", on("claude_over", 60))
        el:registerEventHandler("button_up", on("claude_up", 90))
        el:registerEventHandler("button_disable", on("claude_up", 90))
    end
    bar:registerEventHandler("button_over_disable", on("claude_locked", 60))
    point:registerEventHandler("button_over_disable", on("claude_up", 60))
    bg:addElement(bar)
    bg:addElement(point)
end

if LUI.H1ButtonBackground and type(LUI.H1ButtonBackground.new) == "function" then
    local orig_bg_new = LUI.H1ButtonBackground.new
    LUI.H1ButtonBackground.new = function(...)
        local bg = orig_bg_new(...)
        if bg then
            local ok, err = pcall(row_marks, bg)
            if not ok then
                log("row: " .. tostring(err))
            end
        end
        return bg
    end
end

----------------------------------------------------------------------------
-- 4. Every menu: the shade down its left, the lit band over the footer.
----------------------------------------------------------------------------

local function insert_behind(menu, el)
    local ok = pcall(function()
        menu:insertElement(el, 1)
    end)
    if not ok then
        ok = pcall(function()
            menu:insertElement(el, 0)
        end)
    end
    return ok
end

local function decorate(menu)
    if not in_frontend then
        -- (In a match the pause menu sits over the game: no shade.)
        return
    end
    -- The shade, in three steps (a fade to the right).
    for i, step in ipairs({ { 0, 0.30 }, { 1, 0.18 }, { 2, 0.10 } }) do
        local w = UI.px(560) + step[1] * UI.px(170)
        insert_behind(menu, LUI.UIImage.new({
            topAnchor = true,
            bottomAnchor = true,
            leftAnchor = true,
            rightAnchor = false,
            top = -200,
            bottom = 200,
            left = -400,
            width = w + 400,
            material = UI.material("white"),
            color = UI.SHADE,
            alpha = step[2],
        }))
    end
    -- The lit band: grey into the accent, across the left of the screen.
    local band = LUI.UIElement.new({
        topAnchor = false,
        bottomAnchor = true,
        leftAnchor = true,
        rightAnchor = false,
        bottom = -UI.px(66),
        height = math.max(2, UI.px(5)),
        left = -200,
        width = 900,
    })
    local pieces = {
        { 0, 360, { r = 0.55, g = 0.58, b = 0.62 }, 0.45 },
        { 360, 340, UI.ACCENT, 0.55 },
        { 700, 200, UI.ACCENT, 0.22 },
    }
    for _, p in ipairs(pieces) do
        band:addElement(LUI.UIImage.new({
            topAnchor = true,
            bottomAnchor = true,
            leftAnchor = true,
            rightAnchor = false,
            top = 0,
            bottom = 0,
            left = p[1],
            width = p[2],
            material = UI.material("white"),
            color = p[3],
            alpha = p[4],
        }))
    end
    menu:addElement(band)
end

if LUI.MenuTemplate and type(LUI.MenuTemplate.new) == "function" then
    local orig_template_new = LUI.MenuTemplate.new
    LUI.MenuTemplate.new = function(...)
        local menu = orig_template_new(...)
        if menu then
            local ok, err = pcall(decorate, menu)
            if not ok then
                log("menu: " .. tostring(err))
            end
        end
        return menu
    end
end

----------------------------------------------------------------------------
-- 5. The scoreboard's own colours (LUI.Scoreboard's, read as it builds).
----------------------------------------------------------------------------

if not in_frontend then
    local function paint(c, to)
        if type(c) == "table" and type(c.r) == "number" then
            c.r, c.g, c.b = to.r, to.g, to.b
        end
    end
    for _, sb in ipairs({ LUI.Scoreboard or false, LUI.mp_hud and LUI.mp_hud.Scoreboard or false }) do
        if type(sb) == "table" then
            pcall(function()
                paint(sb.RowGrayColor, UI.PANEL)
                paint(sb.PlayerNameHighlightColor, UI.GOLD)
                paint(sb.RankNumColor, UI.GOLD)
            end)
        end
    end
end

----------------------------------------------------------------------------
-- 6. The front end in our shell's words and order (shell/mod.rs): the main
-- menu's PLAY ONLINE is MULTIPLAYER; in Multiplayer, FIND A MATCH (HMW's
-- server browser) and CUSTOM GAME (private match) come first, then
-- Create-a-Class, Barracks and the rest as HMW has them.
----------------------------------------------------------------------------

if in_frontend then
    local labels = {
        PLATFORM_PLAY_ONLINE = "MULTIPLAYER",
        LUA_MENU_SERVERLIST = "FIND A MATCH",
        MENU_PRIVATE_MATCH = "CUSTOM GAME",
    }
    for key, text in pairs(labels) do
        pcall(function()
            game:addlocalizedstring(key, text)
        end)
    end

    local builders = LUI.MenuBuilder and LUI.MenuBuilder.m_types_build
    local stock = builders and builders.menu_xboxlive
    if stock then
        builders.menu_xboxlive = function(...)
            local menu = stock(...)
            pcall(function()
                local list = menu and menu.list
                local btn = list and list:getChildById("menu_xboxlive_private_match")
                if btn then
                    list:removeElement(btn)
                    if not pcall(function()
                        list:insertElement(btn, 2)
                    end) then
                        list:addElement(btn)
                    end
                end
            end)
            return menu
        end
    end
end

log("loaded")
