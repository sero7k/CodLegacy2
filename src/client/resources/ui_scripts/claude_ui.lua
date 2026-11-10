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
        local line = "[claude_ui] " .. tostring(msg)
        print(line)
        if game and game.claudelog then
            game:claudelog(line)
        end
    end)
end

local in_frontend = false
pcall(function()
    in_frontend = Engine.InFrontend() and true or false
end)

-- Whether the front end (not a match) is up right now. The script loads once,
-- so this is asked each time rather than remembered.
local function frontend_now()
    local ok, v = pcall(function()
        return Engine.InFrontend()
    end)
    if ok then
        return v and true or false
    end
    return in_frontend
end

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
    if c.g > c.r + 0.06 and c.g >= c.b + 0.02 then
        return true
    end
    -- HMW's accent is yellow / gold: that becomes our blue too.
    return c.r >= 0.5 and c.g >= 0.4 and c.b <= c.g - 0.2 and math.abs(c.r - c.g) <= 0.3
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

-- H1's green art (pixels, not tints): flat accent, or left out.
rule({ "h1_ui_divider_green", "h1_ui_progressbar_green" }, { color = UI.ACCENT, alpha = 0.9 })
rule({
    "h1_ui_deco_green_triangle", "h1_ui_topbar_title_bg", "h1_ui_topbar_titletext_bg_not_tiled",
    "h1_title_background", "logo_game_big_glow",
}, { hide = true })

-- HMW's own (h2_) menu art: the rows, the title band, the dividers.
rule({ "h2_btn_unfocused" }, { color = UI.BLACK, alpha = 0.45 })
rule({ "h2_btn_unfocused_locked" }, { color = UI.BLACK, alpha = 0.3 })
rule({ "h2_btn_focused_fill" }, { hide = true })
rule({ "h2_btn_focused_locked" }, { color = UI.ACCENT, alpha = 0.35 })
rule({
    "h2_btn_focused_stroke", "h2_btn_focused_outerglow", "h2_btn_dot_pattern", "btn_focused_rect_innerglow",
    "h2_title_splatter_01", "h2_title_splatter_02", "h2_title_smoke_01", "h2_title_smoke_02",
    "h2_title_pattern_dot", "h2_title_backglow_pc", "h2_title_backglow_light", "h2_title_arrow",
    "h1_deco_title_point", "h1_deco_double_triangle", "h1_deco_scanlines_overlay",
}, { hide = true })
rule({ "h2_ui_divider_yellow", "h1_ui_divider_gradient_left", "h1_ui_frame_green" }, { color = UI.ACCENT, alpha = 0.85 })

-- HMW's HUD boxes.
rule({ "h2m_hud_weapwidget_blur", "h2m_hud_as_kbm_blur" }, { color = UI.PANEL, alpha = 0.72, ingame = true })
rule({ "h2m_hud_weapwidget_border", "h2m_hud_as_kbm_border" }, { hide = true, ingame = true })

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
    if r and r.ingame and frontend_now() then
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

local seen_materials, seen_count = {}, 0
local function note_material(m)
    if seen_count >= 250 or m == nil or seen_materials[m] then
        return
    end
    seen_materials[m] = true
    seen_count = seen_count + 1
    if frontend_now() then
        local n = names[m]
        log("material: " .. tostring(n or "?") .. (n and RULES[n] and " (ruled)" or ""))
    end
end

local orig_ras = LUI.UIElement.registerAnimationState
local function our_ras(self, name, state, ...)
    if type(state) == "table" and state.material ~= nil then
        pcall(note_material, state.material)
        pcall(function()
            if names[state.material] == "h2m_ui_playercard_bg" then
                UI.card_bg = self
            end
        end)
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
    -- The band in focus (our accent), driven by the row's own focus events so
    -- only the row in focus is lit.
    local band = LUI.UIImage.new({
        topAnchor = true, bottomAnchor = true, leftAnchor = true, rightAnchor = true,
        top = 0, bottom = 0, left = 0, right = 0,
        material = UI.material("white"),
        color = UI.ACCENT,
        alpha = 0,
    })
    band:registerAnimationState("claude_over", {
        topAnchor = true, bottomAnchor = true, leftAnchor = true, rightAnchor = true,
        top = 0, bottom = 0, left = 0, right = 0,
        color = UI.ACCENT,
        alpha = 0.88,
    })
    band:registerAnimationState("claude_up", {
        topAnchor = true, bottomAnchor = true, leftAnchor = true, rightAnchor = true,
        top = 0, bottom = 0, left = 0, right = 0,
        color = UI.ACCENT,
        alpha = 0,
    })
    band:registerEventHandler("button_over", function(el) pcall(function() el:animateToState("claude_over", 60) end) end)
    band:registerEventHandler("button_up", function(el) pcall(function() el:animateToState("claude_up", 90) end) end)
    band:registerEventHandler("button_disable", function(el) pcall(function() el:animateToState("claude_up", 0) end) end)
    band:registerEventHandler("lose_focus", function(el) pcall(function() el:animateToState("claude_up", 90) end) end)
    bg:addElement(band)

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

-- Our menus (view.rs / ghosts.rs rows): the text sits at the row's right end,
-- not its left. H1's AddButton passes text_align_without_content = Left, so
-- it is overridden through the button's own properties (front end only).
if LUI.MenuTemplate and type(LUI.MenuTemplate.AddButton) == "function" and LUI.Alignment then
    local orig_add_button = LUI.MenuTemplate.AddButton
    LUI.MenuTemplate.AddButton = function(menu, a1, a2, a3, a4, a5, props, a7)
        if frontend_now() then
            pcall(function()
                props = props or {}
                if props.text_align_without_content == nil then
                    props.text_align_without_content = LUI.Alignment.Right
                    props.text_padding_without_content = UI.px(36)
                    props.text_padding_focus_without_content = UI.px(36)
                end
            end)
        end
        UI.in_add_button = true
        local res = { pcall(orig_add_button, menu, a1, a2, a3, a4, a5, props, a7) }
        UI.in_add_button = false
        if not res[1] then
            error(res[2], 0)
        end
        return unpack(res, 2)
    end
    log("rows right-aligned")
end

-- The one place a button's label gets its alignment (every button, however it
-- was built): left-aligned labels in the front end go to the right end.
if LUI.UIGenericButton and type(LUI.UIGenericButton.ButtonLabelFactory) == "function" and LUI.Alignment then
    local orig_label = LUI.UIGenericButton.ButtonLabelFactory
    local logged = false
    LUI.UIGenericButton.ButtonLabelFactory = function(settings, ...)
        if UI.in_add_button and frontend_now() and type(settings) == "table" and settings.align == LUI.Alignment.Left then
            local ok, copy = pcall(UI.copy, settings)
            if ok and type(copy) == "table" then
                copy.align = LUI.Alignment.Right
                copy.padding = UI.px(36)
                copy.paddingFocus = UI.px(36)
                settings = copy
                if not logged then
                    logged = true
                    log("label factory: right-aligned a button label")
                end
            end
        end
        return orig_label(settings, ...)
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

-- `unlockall` (the console command sets the flag): the level goes to the
-- maximum from here, where the game's menus can write the player's data.
local function add_unlock_poll(menu)
    pcall(function()
        menu:addElement(LUI.UITimer.new(500, "claude_unlock_poll"))
        menu:registerEventHandler("claude_unlock_poll", function()
            pcall(function()
                if Engine.GetDvarInt("clc_unlockall_run") ~= 1 then
                    return
                end
                Engine.SetDvarFromString("clc_unlockall_run", "0")
                local prestige = Engine.GetPlayerData(0, CoD.StatsGroup.Ranked, "prestige") or 0
                local max = Rank.GetMaxRank(prestige)
                local xp = max <= 0 and 0 or Rank.GetRankMaxXP(max - 1, prestige)
                Engine.SetPlayerData(0, CoD.StatsGroup.Ranked, "experience", xp)
                log("unlockall: level set to max")
            end)
        end)
    end)
end

-- After a menu has built its rows (whatever built them): each row's label
-- goes to the right end of its row, as our shell's rows (ghosts.rs). The
-- label's default and focus states are registered again, right-aligned.
local logged_trees = {}
local function walk(el, depth, fn)
    local c = el:getFirstChild()
    while c do
        fn(c, depth)
        if depth < 12 then
            walk(c, depth + 1, fn)
        end
        c = c:getNextSibling()
    end
end

local function realign_label(label)
    local ts = CoD.TextSettings.BodyFont or CoD.TextSettings.TitleFontTiny
    local h = UI.px(30)
    local y = 0
    local function st(c, a)
        return {
            alignment = LUI.Alignment.Right,
            font = ts.Font,
            topAnchor = false,
            bottomAnchor = false,
            leftAnchor = true,
            rightAnchor = true,
            left = 0,
            right = -UI.px(36),
            top = y - h / 2,
            bottom = y + h / 2,
            red = c.r,
            green = c.g,
            blue = c.b,
            alpha = a,
        }
    end
    label:registerAnimationState("default", st(UI.TEXT, 0.82))
    label:registerAnimationState("focus", st(UI.WHITE, 1))
    label:animateToState("default")
end

local function after_build(menu)
    local key = tostring(menu.id or menu.type or "?")
    local count = 0
    walk(menu, 0, function(el, depth)
        local id = tostring(el.id or "")
        if id == "text_label" then
            local parent = el:getParent()
            -- (Only list rows: buttons inside the menu's list.)
            local in_list = false
            local p = parent
            while p do
                if p == menu.list then
                    in_list = true
                    break
                end
                p = p:getParent()
            end
            if in_list then
                local ok, err = pcall(realign_label, el)
                if ok then
                    count = count + 1
                else
                    log("realign: " .. tostring(err))
                end
            end
        end
    end)
    pcall(function()
        local title = menu:getChildById("generic_title")
        if title and not title.claude_bar then
            title.claude_bar = true
            local function find(root, id)
                local hit = nil
                walk(root, 0, function(el)
                    if not hit and el.id == id then
                        hit = el
                    end
                end)
                return hit
            end
            local crumb = find(title, "breadcrumb_text")
            local head = find(title, "header_text")
            local function text_of(el)
                local ok, t = pcall(function()
                    return el:getText()
                end)
                return ok and type(t) == "string" and t or ""
            end
            local line = text_of(head)
            local c = text_of(crumb)
            log("title texts: crumb='" .. c .. "' head='" .. line .. "' in " .. tostring(menu.id))
            if c ~= "" and line ~= "" then
                line = c .. "  /  " .. line
            elseif line == "" then
                line = c
            end
            -- (Coordinates inside generic_title, whose corner sits at about
            -- 101, 43 on a 1080-line screen.)
            local bar = LUI.UIImage.new({
                topAnchor = true,
                bottomAnchor = false,
                leftAnchor = true,
                rightAnchor = false,
                top = UI.px(45),
                height = UI.px(40),
                left = UI.px(-1),
                width = math.max(3, UI.px(7)),
                material = UI.material("white"),
                red = UI.ACCENT.r,
                green = UI.ACCENT.g,
                blue = UI.ACCENT.b,
                alpha = 1,
            })
            bar.claude = true
            title:addElement(bar)
            if line ~= "" then
                for _, el in ipairs({ crumb, head }) do
                    if el then
                        pcall(function()
                            el:setAlpha(0)
                        end)
                    end
                end
                local font = (CoD.TextSettings.BodyFont or CoD.TextSettings.TitleFontTiny).Font
                local t = LUI.UIText.new({
                    topAnchor = true,
                    bottomAnchor = false,
                    leftAnchor = true,
                    rightAnchor = false,
                    top = UI.px(41),
                    height = UI.px(48),
                    left = UI.px(21),
                    width = UI.px(1400),
                    alignment = LUI.Alignment.Left,
                    font = font,
                    red = 1,
                    green = 1,
                    blue = 1,
                    alpha = 1,
                })
                t:setText(line)
                t.claude = true
                title:addElement(t)
                local ticks = 0
                title:addElement(LUI.UITimer.new(150, "claude_title_tick"))
                title:registerEventHandler("claude_title_tick", function()
                    ticks = ticks + 1
                    if ticks > 40 then
                        return
                    end
                    pcall(function()
                        local l2 = text_of(head)
                        local c2 = text_of(crumb)
                        if c2 ~= "" and l2 ~= "" then
                            l2 = c2 .. "  /  " .. l2
                        elseif l2 == "" then
                            l2 = c2
                        end
                        if l2 ~= "" then
                            t:setText(l2)
                        end
                        if crumb then crumb:setAlpha(0) end
                        if head then head:setAlpha(0) end
                    end)
                end)
            end
        end
    end)
    if not logged_trees[key] then
        logged_trees[key] = true
        log("menu '" .. key .. "': realigned " .. count .. " row labels")
        local lines = 0
        walk(menu, 0, function(el, depth)
            if lines < 60 and depth <= 4 then
                lines = lines + 1
                log("  tree " .. string.rep("  ", depth) .. tostring(el.id or "-"))
            end
        end)
    end
end

-- Our backdrop in place of the lobby room and its soldier (every front-end
-- menu): our shell's pictures (codlegacy/backgrounds/bgNN.jpg beside the
-- game), one at a time, drifting slowly closer and fading into the next.
local BACKDROP_COUNT = 21
local BACKDROP_HOLD = 9000
local BACKDROP_FADE = 900

local function backdrop_wanted(key)
    -- Every front-end screen, as our shell (its screens all sit on the
    -- backdrop and none shows the lobby room; Create-a-Class shows the gun's
    -- picture, not the 3D model).
    return key ~= "?"
end

local function backdrop_materials()
    if UI.backdrops then
        return UI.backdrops
    end
    local list = {}
    if game and game.clcimage then
        for i = 1, BACKDROP_COUNT do
            local ok, m = pcall(function()
                return game:clcimage(string.format("codlegacy/backgrounds/bg%02d.jpg", i))
            end)
            if ok and m then
                list[#list + 1] = m
            end
        end
    end
    UI.backdrops = list
    log("backdrop: " .. #list .. " pictures")
    return list
end

local function add_backdrop(menu)
    local pics = backdrop_materials()
    if #pics == 0 then
        return
    end
    local holder = LUI.UIElement.new({
        topAnchor = true, bottomAnchor = true, leftAnchor = true, rightAnchor = true,
        top = -40, bottom = 40, left = -70, right = 70,
    })
    holder.claude = true
    -- Black first: the room and its soldier are covered even between pictures.
    holder:addElement(LUI.UIImage.new({
        topAnchor = true, bottomAnchor = true, leftAnchor = true, rightAnchor = true,
        top = -20, bottom = 20, left = -20, right = 20,
        material = UI.material("black") or UI.material("white"),
        red = 0, green = 0, blue = 0,
        alpha = 1,
    }))
    -- (The pictures are 16:9 with our shell's fades baked in.)
    local over = 0
    local function frame(zoom, a)
        return {
            topAnchor = true, bottomAnchor = true, leftAnchor = true, rightAnchor = true,
            top = -over - zoom * 0.56, bottom = over + zoom * 0.56,
            left = -zoom, right = zoom,
            alpha = a,
        }
    end
    local layers = {}
    for n = 1, 2 do
        local img = LUI.UIImage.new(frame(0, 0))
        img:setImage(pics[1])
        img:registerAnimationState("start", frame(0, 0))
        img:registerAnimationState("in", frame(12, 1))
        img:registerAnimationState("drift", frame(60, 1))
        img:registerAnimationState("out", frame(70, 0))
        img:registerEventHandler("transition_complete_in", function(el)
            pcall(function()
                el:animateToState("drift", BACKDROP_HOLD)
            end)
        end)
        holder:addElement(img)
        layers[n] = img
    end
    UI.backdrop_index = UI.backdrop_index or 0
    local front = 1
    local function show_next()
        -- A random picture, never the same one twice in a row (as our shell).
        local i = math.random(1, #pics)
        if #pics > 1 and i == UI.backdrop_index then
            i = (i % #pics) + 1
        end
        UI.backdrop_index = i
        local incoming = layers[front]
        local outgoing = layers[3 - front]
        front = 3 - front
        incoming:setImage(pics[UI.backdrop_index])
        incoming:animateToState("start", 0)
        incoming:animateToState("in", BACKDROP_FADE)
        outgoing:animateToState("out", BACKDROP_FADE)
    end
    holder:addElement(LUI.UITimer.new(BACKDROP_HOLD, "claude_backdrop_next"))
    holder:registerEventHandler("claude_backdrop_next", function()
        pcall(show_next)
    end)
    pcall(function()
        math.randomseed(Engine.GetMilliseconds and Engine.GetMilliseconds() or os.time())
    end)
    pcall(show_next)
    -- Over the menu's own background art (the main menu's picture), under
    -- its title and rows: just before the title.
    local placed = false
    pcall(function()
        local idx, c = 0, menu:getFirstChild()
        while c do
            idx = idx + 1
            if c.id == "generic_title" then
                menu:insertElement(holder, idx)
                placed = true
                break
            end
            c = c:getNextSibling()
        end
    end)
    if not placed then
        insert_behind(menu, holder)
    end
end

-- What is left of HMW's old look: the title's art (backgrounds, glow, dots,
-- arrow, the dividers), the list's dividers and, where our backdrop is up,
-- the menu's own background art. Our own pieces carry `claude`.
local function hide(el)
    if el and not el.claude then
        pcall(function()
            el:setAlpha(0)
        end)
    end
end

local function sweep_old_look(menu, backdrop)
    local title = menu:getChildById("generic_title")
    if title then
        local c = title:getFirstChild()
        while c do
            if c.id ~= "generic_menu_title_id" then
                hide(c)
            end
            c = c:getNextSibling()
        end
    end
    hide(menu.listDivider)
    if menu.list then
        hide(menu.list.bottomDivider)
    end
    -- After the title: unnamed pieces made only of pictures are the title's
    -- divider lines (HMW's); anything holding text (the player card) stays.
    local function only_images(el)
        local c = el:getFirstChild()
        if not c then
            return false
        end
        while c do
            if c.id ~= "LUIImage" then
                return false
            end
            c = c:getNextSibling()
        end
        return true
    end
    do
        local after = false
        local c = menu:getFirstChild()
        while c do
            if after and not c.claude then
                if c.id == "LUIImage" or (c.id == "LUIElement" and only_images(c)) then
                    hide(c)
                end
            end
            if c.id == "generic_title" then
                after = true
            end
            c = c:getNextSibling()
        end
    end
    if backdrop then
        -- (The menu's children before its title are its background art.)
        local c = menu:getFirstChild()
        while c and c.id ~= "generic_title" do
            hide(c)
            c = c:getNextSibling()
        end
    end
end

-- No build number over the title.
if LUI.MenuTemplate then
    LUI.MenuTemplate.AddBuildNumber = function() end
end

-- The player banner at the top right, as our shell's (view.rs card): a line
-- of his numbers over it, his calling card as the banner with his name and
-- level at its right, a blue strip along its foot with LEVEL / XP and his
-- progress, his emblem standing beside it, and his line in the party under
-- it. HMW's own card (found by its picture) is hidden.
local function stat(group, name)
    local ok, v = pcall(function()
        return Engine.GetPlayerData(0, group, name)
    end)
    return ok and tonumber(v) or 0
end

local function player_info()
    local p = {}
    p.name = "PLAYER"
    pcall(function()
        p.name = Engine.GetUsernameByController(0) or p.name
    end)
    p.prestige = stat(CoD.StatsGroup.Ranked, "prestige")
    p.xp = 0
    pcall(function()
        p.xp = tonumber(Engine.GetPlayerDataMPXP(0)) or 0
    end)
    p.rank = 0
    pcall(function()
        p.rank = Lobby.GetRankForXP(p.xp, p.prestige) or 0
    end)
    p.level = tostring(p.rank + 1)
    pcall(function()
        p.level = tostring(Rank.GetRankDisplay(p.rank))
    end)
    p.frac = 0
    pcall(function()
        local lo = Rank.GetRankMinXP(p.rank, p.prestige)
        local hi = Rank.GetRankMaxXP(p.rank, p.prestige)
        if hi and lo and hi > lo then
            p.frac = math.max(0, math.min(1, (p.xp - lo) / (hi - lo)))
        end
    end)
    p.kills = stat(CoD.StatsGroup.Ranked, "kills")
    p.deaths = stat(CoD.StatsGroup.Ranked, "deaths")
    p.kd = p.deaths > 0 and p.kills / p.deaths or p.kills
    p.rank_icon = nil
    pcall(function()
        p.rank_icon = UI.material(Rank.GetRankIcon(p.rank, p.prestige))
    end)
    p.card = nil
    pcall(function()
        local idx = Engine.GetPlayerData(0, CoD.StatsGroup.Common, "callingCardIndex")
        local img = Engine.TableLookupByRow(CallingCardTable.File, tonumber(idx), CallingCardTable.Cols.Image)
        if img and img ~= "" then
            p.card = UI.material(img)
        end
    end)
    p.emblem = nil
    pcall(function()
        local idx = Engine.GetPlayerData(0, CoD.StatsGroup.Common, "emblemPatchIndex")
        local img = Engine.TableLookup(EmblemIconTable.File, EmblemIconTable.Cols.ID, tostring(idx), EmblemIconTable.Cols.Image)
        if img and img ~= "" then
            p.emblem = UI.material(img)
        end
    end)
    return p
end

-- Our custom calling cards and emblems (codlegacy/cards, codlegacy/emblems
-- beside the game): flipbooks, frames stacked top to bottom, played at 12 a
-- second as in our shell. Chosen with clc_card / clc_emblem (saved); a click
-- on the banner or the emblem goes to the next one ("" is the game's own).
local CUSTOM_CARDS = {
    { "card_above_the_clouds", 19 }, { "card_freedom", 36 }, { "card_awakened_eye", 51 },
    { "card_the_founder", 60 }, { "card_dread", 60 }, { "card_the_rumbling", 60 },
    { "card_third_impact", 25 }, { "card_frieren_himmel", 36 }, { "card_himmel", 36 },
    { "card_stark", 36 }, { "card_campfire", 1 }, { "card_quiet_forest", 24 },
    { "card_frieren", 28 }, { "card_kittens_look_up", 1 }, { "card_sleepy_paws", 1 },
    { "card_snuggle", 1 }, { "card_mural", 1 },
}
local CUSTOM_EMBLEMS = { { "emblem_legacy", 1 }, { "emblem_berserk", 18 } }
local FLIP_MS = 83

local function custom_entry(list, name)
    for i, e in ipairs(list) do
        if e[1] == name then
            return e, i
        end
    end
    return nil, 0
end

local function next_custom(dvar, list)
    local cur = ""
    pcall(function()
        cur = Engine.GetDvarString(dvar) or ""
    end)
    local _, i = custom_entry(list, cur)
    local nxt = list[i + 1] and list[i + 1][1] or ""
    pcall(function()
        Engine.SetDvarFromString(dvar, nxt)
    end)
    log(dvar .. " = '" .. nxt .. "'")
end

local function custom_frame(kind, entry, frame)
    local fh = kind == "cards" and 128 or 256
    local ok, m = pcall(function()
        return game:clcimage("codlegacy/" .. kind .. "/" .. entry[1] .. ".png", frame % entry[2], fh)
    end)
    return ok and m or nil
end

-- An image element showing the custom chosen in `dvar` (or `fallback`).
local function custom_image(el, kind, list, dvar, fallback)
    local frame, shown = 0, nil
    local function update()
        local name = ""
        pcall(function()
            name = Engine.GetDvarString(dvar) or ""
        end)
        local entry = custom_entry(list, name)
        if entry then
            local m = custom_frame(kind, entry, frame)
            if m then
                el:setImage(m)
                el:setAlpha(1)
                shown = name
                frame = frame + 1
                return
            end
        end
        local fb = fallback()
        if shown ~= "" or fb then
            if fb then
                el:setImage(fb)
                el:setAlpha(1)
            else
                el:setAlpha(0)
            end
            shown = ""
        end
    end
    el:addElement(LUI.UITimer.new(FLIP_MS, "claude_flip_" .. kind))
    el:registerEventHandler("claude_flip_" .. kind, function()
        pcall(update)
    end)
    pcall(update)
end

-- A click area (the banner, the emblem) that runs `fn`.
local function click_area(parent, st, fn)
    local area = LUI.UIElement.new(st)
    area:setHandleMouseButton(true)
    area:registerEventHandler("leftmouseup", function()
        pcall(fn)
    end)
    parent:addElement(area)
    return area
end

local function build_banner(menu)
    local ox, oy = menu:getCurrentGlobalRect()
    ox, oy = ox or 0, oy or 0
    local body = (CoD.TextSettings.BodyFont or CoD.TextSettings.TitleFontTiny).Font
    local bold = (CoD.TextSettings.BodyFontBold or CoD.TextSettings.BodyFont or CoD.TextSettings.TitleFontTiny).Font
    local function box(x1, y1, x2, y2)
        return {
            topAnchor = true, leftAnchor = true, bottomAnchor = false, rightAnchor = false,
            left = UI.px(x1) - ox, top = UI.px(y1) - oy,
            width = UI.px(x2 - x1), height = UI.px(y2 - y1),
        }
    end
    local function image(parent, x1, y1, x2, y2, mat, c, a)
        local st = box(x1, y1, x2, y2)
        st.material = mat or UI.material("white")
        st.color = c or UI.WHITE
        st.alpha = a or 1
        local el = LUI.UIImage.new(st)
        parent:addElement(el)
        return el
    end
    local function text(parent, x1, y, h, str, font, c, a, align, w)
        local st = box(x1, y, x1 + (w or 600), y + h)
        st.font = font
        st.alignment = align or LUI.Alignment.Left
        st.color = c or UI.WHITE
        st.alpha = a or 1
        local el = LUI.UIText.new(st)
        el:setText(str)
        parent:addElement(el)
        return el
    end

    local root = LUI.UIElement.new({
        topAnchor = true, bottomAnchor = true, leftAnchor = true, rightAnchor = true,
        top = 0, bottom = 0, left = 0, right = 0,
    })
    root.claude = true
    local p = player_info()
    local t = {}

    -- His numbers over the banner.
    image(root, 1138, 78, 1162, 98, p.rank_icon, UI.WHITE, p.rank_icon and 1 or 0)
    t.level_small = text(root, 1166, 76, 24, p.level, bold, UI.WHITE, 1)
    t.kills = text(root, 1200, 76, 24, tostring(p.kills), bold, UI.WHITE, 1)
    text(root, 1214 + string.len(tostring(p.kills)) * 6, 82, 16, "KILLS", body, UI.DIM, 0.6)
    t.deaths = text(root, 1268, 76, 24, tostring(p.deaths), bold, UI.WHITE, 1)
    text(root, 1282 + string.len(tostring(p.deaths)) * 6, 82, 16, "DEATHS", body, UI.DIM, 0.6)
    t.kd = text(root, 1345, 76, 24, string.format("%.2f", p.kd), bold, UI.WHITE, 1)
    text(root, 1392, 82, 16, "K/D RATIO", body, UI.DIM, 0.6)

    -- The banner: his calling card, a white rule along its top.
    image(root, 1130, 104, 1708, 206, UI.material("white"), UI.PANEL, 0.9)
    local card_img = image(root, 1130, 104, 1708, 206, p.card or UI.material("white"), UI.WHITE, p.card and 1 or 0)
    pcall(custom_image, card_img, "cards", CUSTOM_CARDS, "clc_card", function()
        return player_info().card
    end)
    image(root, 1130, 103, 1708, 105, UI.material("white"), UI.WHITE, 0.9)
    t.name = text(root, 1300, 118, 34, string.upper(p.name), bold, UI.WHITE, 1, LUI.Alignment.Right, 260)
    t.level_big = text(root, 1570, 118, 34, p.level, bold, UI.WHITE, 1, LUI.Alignment.Left, 80)
    -- The strip along its foot.
    image(root, 1130, 162, 1708, 206, UI.material("white"), UI.ACCENT, 0.82)
    t.xp = text(root, 1139, 171, 20, "LEVEL " .. p.level .. "  \194\183  " .. tostring(p.xp) .. " XP", bold, UI.WHITE, 1)
    image(root, 1139, 193, 1700, 196, UI.material("white"), UI.WHITE, 0.25)
    t.bar = image(root, 1139, 193, 1139 + math.max(1, math.floor(561 * p.frac)), 196, UI.material("white"), UI.WHITE, 1)
    -- His emblem beside it.
    image(root, 1717, 104, 1818, 206, UI.material("white"), UI.PANEL, 0.9)
    local emblem_img = image(root, 1717, 104, 1818, 206, p.emblem or p.rank_icon or UI.material("white"), UI.WHITE, (p.emblem or p.rank_icon) and 1 or 0)
    pcall(custom_image, emblem_img, "emblems", CUSTOM_EMBLEMS, "clc_emblem", function()
        local q = player_info()
        return q.emblem or q.rank_icon
    end)
    -- Clicks: the banner changes the card, the emblem the emblem.
    pcall(click_area, root, box(1130, 104, 1708, 206), function()
        next_custom("clc_card", CUSTOM_CARDS)
    end)
    pcall(click_area, root, box(1717, 104, 1818, 206), function()
        next_custom("clc_emblem", CUSTOM_EMBLEMS)
    end)

    -- His line in the party.
    text(root, 1176, 236, 20, ">", body, UI.WHITE, 1)
    image(root, 1202, 234, 1226, 256, p.rank_icon, UI.WHITE, p.rank_icon and 1 or 0)
    t.party_level = text(root, 1236, 234, 24, p.level, body, UI.WHITE, 0.9)
    t.party_name = text(root, 1290, 234, 24, p.name, body, UI.WHITE, 0.9)
    image(root, 1178, 261, 1758, 262, UI.material("white"), UI.WHITE, 0.25)

    -- Kept current (a match, a prestige, a new card).
    root:addElement(LUI.UITimer.new(2000, "claude_banner_tick"))
    root:registerEventHandler("claude_banner_tick", function()
        pcall(function()
            local q = player_info()
            t.level_small:setText(q.level)
            t.kills:setText(tostring(q.kills))
            t.deaths:setText(tostring(q.deaths))
            t.kd:setText(string.format("%.2f", q.kd))
            t.name:setText(string.upper(q.name))
            t.level_big:setText(q.level)
            t.xp:setText("LEVEL " .. q.level .. "  \194\183  " .. tostring(q.xp) .. " XP")
            t.party_level:setText(q.level)
            t.party_name:setText(q.name)
        end)
    end)
    menu:addElement(root)
    return root
end

local banner_logged = false
local function place_card(menu)
    local bg = UI.card_bg
    if not bg then
        return
    end
    local card = bg:getParent()
    if not card then
        return
    end
    local inside, p = false, card
    while p do
        if p == menu then
            inside = true
            break
        end
        p = p:getParent()
    end
    if not inside then
        return
    end
    -- HMW's card and the pieces drawn with it (its rule above) go.
    hide(card)
    local parent = card:getParent()
    if parent then
        local c = parent:getFirstChild()
        while c do
            if c.id == "LUIImage" then
                hide(c)
            end
            c = c:getNextSibling()
        end
    end
    if not menu.claude_banner then
        local ok, err = pcall(build_banner, menu)
        menu.claude_banner = true
        if not banner_logged then
            banner_logged = true
            log("banner: " .. (ok and "built" or tostring(err)))
        end
    end
end

local function add_after_build(menu)
    local key0 = tostring(menu.id or menu.type or "?")
    local sweeps = 0
    menu:addElement(LUI.UITimer.new(120, "claude_sweep_tick"))
    menu:registerEventHandler("claude_sweep_tick", function()
        sweeps = sweeps + 1
        if sweeps <= 50 or sweeps % 10 == 0 then
            pcall(sweep_old_look, menu, backdrop_wanted(key0))
        end
    end)
    pcall(function()
        local key = tostring(menu.id or menu.type or "?")
        if backdrop_wanted(key) then
            -- The lobby room is behind our backdrop: put it in its "obscured"
            -- mode so its soldier stops playing (and making sound) unseen.
            -- (Create-a-Class pushes its own mode and gives this one back.)
            pcall(function()
                Engine.SetDvarInt("virtualLobbyMode", VirtualLobbyModes.LUI_MODE_OBSCURED)
            end)
            local ok, err = pcall(add_backdrop, menu)
            if not ok then
                log("backdrop: " .. tostring(err))
            end
        end
    end)
    menu:addElement(LUI.UITimer.new(400, "claude_card_tick"))
    menu:registerEventHandler("claude_card_tick", function()
        local ok, err = pcall(place_card, menu)
        if not ok and not card_logged then
            card_logged = true
            log("card: " .. tostring(err))
        end
    end)
    menu:addElement(LUI.UITimer.new(50, "claude_after_build", nil, true))
    menu:registerEventHandler("claude_after_build", function()
        local ok, err = pcall(after_build, menu)
        if not ok then
            log("after_build: " .. tostring(err))
        end
    end)
end

local function decorate(menu)
    if not in_frontend then
        -- (In a match the pause menu sits over the game: no shade.)
        return
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
    band.claude = true
    menu:addElement(band)
    add_unlock_poll(menu)
    pcall(add_after_build, menu)
end

if LUI.MenuTemplate and type(LUI.MenuTemplate.new) == "function" then
    local orig_template_new = LUI.MenuTemplate.new
    LUI.MenuTemplate.new = function(...)
        local menu = orig_template_new(...)
        -- Only the front end's menus get our shade and band; in-match popups
        -- (team choose, class choose, pause) are left as H1 builds them.
        if menu and frontend_now() then
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

    -- The main menu: our title (MAIN MENU). Everything it does is H1's own:
    -- the stock builder runs as it is, only the title it asks for is swapped.
    do
        local mb = LUI.MenuBuilder and LUI.MenuBuilder.m_types_build
        local stock_main = mb and mb.mp_main_menu
        if stock_main then
            mb.mp_main_menu = function(...)
                local template = LUI.MenuTemplate
                local orig_new = template and template.new
                if type(orig_new) ~= "function" then
                    return stock_main(...)
                end
                template.new = function(menu_arg, opts, ...)
                    if type(opts) == "table" then
                        opts.menu_title = "MAIN MENU"
                    end
                    return orig_new(menu_arg, opts, ...)
                end
                local ok, res = pcall(stock_main, ...)
                template.new = orig_new
                if not ok then
                    error(res, 0)
                end
                return res
            end
        end
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

----------------------------------------------------------------------------
-- 6. Every stock MWR (h1_*) weapon in the Create-a-Class slots (embedded so it
-- loads with the client; the loose data/cdata copy is not shipped to the game
-- folder).
----------------------------------------------------------------------------
if in_frontend and Cac and Cac.Weapons then
do
local stock = {
    Primary = {
        weapon_assault = { "h1_m16", "h1_ak47", "h1_m4", "h1_g3", "h1_g36c", "h1_m14", "h1_mp44", "h1_xmlar", "h1_aprast", "h1_augast" },
        weapon_smg = { "h1_mp5", "h1_skorpion", "h1_uzi", "h1_ak74u", "h1_p90", "h1_febsmg", "h1_aprsmg", "h1_augsmg" },
        weapon_sniper = { "h1_m40a3", "h1_m21", "h1_dragunov", "h1_remington700", "h1_barrett", "h1_febsnp", "h1_junsnp" },
        weapon_shotgun = { "h1_winchester1200", "h1_m1014", "h1_kam12", "h1_junsho" },
        weapon_heavy = { "h1_saw", "h1_rpd", "h1_m60e4", "h1_feblmg", "h1_junlmg" },
    },
    Secondary = {
        weapon_pistol = { "h1_beretta", "h1_usp", "h1_colt45", "h1_deserteagle", "h1_deserteagle55", "h1_janpst", "h1_aprpst", "h1_augpst" },
        weapon_projectile = { "h1_rpg" },
    },
}

local names = {}
for _, classes in pairs(stock) do
    for _, list in pairs(classes) do
        for _, n in ipairs(list) do names[n] = true end
    end
end

-- The CAC drops weapons the inventory/unlock tables don't know (Hidden lock
-- state, or a non-Default inventory type). Treat every stock MWR weapon as a
-- normal, unlocked default item so it shows in the slot.
local lock_fixed = 0
pcall(function()
    local orig_lock = Cac.GetItemLockState
    Cac.GetItemLockState = function(a, b, ref, c, d)
        local r1, r2, r3 = orig_lock(a, b, ref, c, d)
        if ref and names[ref] and r1 == "Hidden" then
            lock_fixed = lock_fixed + 1
            return "Unlocked", 0, ""
        end
        return r1, r2, r3
    end
    local orig_type = Engine.Inventory_GetItemTypeByReference
    Engine.Inventory_GetItemTypeByReference = function(ref, ...)
        if ref and names[ref] then
            return Cac.InventoryItemType.Default
        end
        return orig_type(ref, ...)
    end
end)

local added = 0
pcall(function()
    for section, classes in pairs(stock) do
        local lists = Cac.Weapons[section]
        for class, names in pairs(classes) do
            local list = lists and lists[class]
            if type(list) == "table" then
                local have = {}
                for _, e in ipairs(list) do
                    have[e[1]] = true
                end
                for _, name in ipairs(names) do
                    -- (Only weapons the stats table has a row for: without one the
                    -- slot shows a blank row with a checkerboard picture.)
                    local nm = Engine.TableLookup("mp/statstable.csv", 4, name, 3)
                    if not have[name] and nm and nm ~= "" then
                        table.insert(list, { name, 0 })
                        added = added + 1
                    end
                end
            end
        end
    end
end)
-- Which of them the game's stats table actually knows (name / image columns).
pcall(function()
    local known, unknown = {}, {}
    for _, classes in pairs(stock) do
        for _, list in pairs(classes) do
            for _, n in ipairs(list) do
                local nm = Engine.TableLookup("mp/statstable.csv", 4, n, 3)
                if nm and nm ~= "" then
                    known[#known + 1] = n
                else
                    unknown[#unknown + 1] = n
                end
            end
        end
    end
    log("mwr_weapons: in statstable: " .. #known .. " / not in statstable: " .. #unknown .. " [" .. table.concat(unknown, ",") .. "]")
end)
pcall(function() log("mwr_weapons: added " .. added .. " missing stock weapons (hidden-fix hooks installed)") end)

end
end
