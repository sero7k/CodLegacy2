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
rule({ "h1_ui_btn_unfocused", "h1_btn_unfocused" }, { color = UI.SHADE, alpha = 0.28 })
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
rule({ "h2_btn_unfocused" }, { color = UI.SHADE, alpha = 0.28 })
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
                -- (Every copy HMW builds, not only the last: the lobby under a
                -- menu builds its own, and the one on screen must be found.)
                UI.card_bgs = UI.card_bgs or {}
                local known = false
                for _, e in ipairs(UI.card_bgs) do
                    if e == self then
                        known = true
                    end
                end
                if not known then
                    UI.card_bgs[#UI.card_bgs + 1] = self
                    if #UI.card_bgs > 24 then
                        table.remove(UI.card_bgs, 1)
                    end
                end
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
        alpha = 0.8,
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
        color = UI.ACCENT_LIFT,
        alpha = 0,
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
        color = UI.ACCENT_LIFT,
        alpha = 0,
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
        alpha = 0.2,
    })

    -- The thin line along the row's top (our rows' border): faint, and the
    -- lifted accent on the row in focus.
    local line = LUI.UIImage.new({
        topAnchor = true, bottomAnchor = false, leftAnchor = true, rightAnchor = true,
        top = 0, left = 0, right = 0, height = math.max(1, UI.px(1)),
        material = UI.material("white"),
        color = { r = 0.70, g = 0.80, b = 0.95 },
        alpha = 0.14,
    })
    line:registerAnimationState("claude_over", {
        topAnchor = true, bottomAnchor = false, leftAnchor = true, rightAnchor = true,
        top = 0, left = 0, right = 0, height = math.max(1, UI.px(1)),
        color = UI.ACCENT_LIFT, alpha = 0.9,
    })
    line:registerAnimationState("claude_up", {
        topAnchor = true, bottomAnchor = false, leftAnchor = true, rightAnchor = true,
        top = 0, left = 0, right = 0, height = math.max(1, UI.px(1)),
        color = { r = 0.70, g = 0.80, b = 0.95 }, alpha = 0.14,
    })
    line:registerEventHandler("button_over", function(el) pcall(function() el:animateToState("claude_over", 60) end) end)
    line:registerEventHandler("button_up", function(el) pcall(function() el:animateToState("claude_up", 90) end) end)
    line:registerEventHandler("lose_focus", function(el) pcall(function() el:animateToState("claude_up", 90) end) end)
    bg:addElement(line)

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
-- Our screens' words, from the old client (crates/ui/src/shell: mod.rs
-- rows, view.rs headers): each HMW screen's title trail, and for its rows
-- (by the label HMW gives them) our name and the line about it.
local SCREEN_TITLES = {
    mp_main_menu_container = "MAIN MENU",
    menu_xboxlive_container = "MULTIPLAYER  /  MATCH SELECT",
    menu_systemlink_container = "MULTIPLAYER  /  MATCH SELECT",
    menu_xboxlive_privatelobby_container = "CUSTOM GAME  /  LOBBY",
    gamesetup_menu_main_container = "CUSTOM GAME  /  SETTINGS",
    mapsetup_menu_main_container = "CUSTOM GAME  /  SELECT MAP",
    CacMenu_container = "MULTIPLAYER  /  CREATE A CLASS",
    OperatorMenu_container = "MULTIPLAYER  /  BARRACKS",
    killstreak_selection_container = "MULTIPLAYER  /  SCORESTREAKS",
    PrestigeMenu_container = "MULTIPLAYER  /  BARRACKS  /  PRESTIGE",
    ChallengeCategoryMenu_container = "MULTIPLAYER  /  BARRACKS  /  CHALLENGES",
    OpLeaderboardMain_container = "MULTIPLAYER  /  BARRACKS  /  LEADERBOARDS",
    PlayercardMenu_container = "MULTIPLAYER  /  BARRACKS  /  CALLING CARD & EMBLEM",
    EmblemMenu_container = "MULTIPLAYER  /  BARRACKS  /  EMBLEM",
    CallingCardMenu_container = "MULTIPLAYER  /  BARRACKS  /  CALLING CARD",
    CombatRecordItemListDetails_container = "MULTIPLAYER  /  BARRACKS  /  COMBAT RECORD",
    CombatRecordMedalsDetails_container = "MULTIPLAYER  /  BARRACKS  /  COMBAT RECORD",
}

local ROW_WORDS = {
    ["MULTIPLAYER"] = { "MULTIPLAYER", "Find a match, or a custom game with bots on every map." },
    ["OPTIONS"] = { "OPTIONS", "Graphics, sound and controls." },
    ["QUIT"] = { "QUIT", "Back to the desktop." },
    ["FIND A MATCH"] = { "FIND A MATCH", "Public matches by playlist, and the server's active lobbies." },
    ["CUSTOM GAME"] = { "CUSTOM GAME", "Pick the map, the mode and the bots, then play." },
    ["PRIVATE MATCH"] = { "CUSTOM GAME", "Pick the map, the mode and the bots, then play." },
    ["CREATE A CLASS"] = { "CREATE A CLASS", "Build your loadouts." },
    ["BARRACKS"] = { "BARRACKS", "Your combat record, calling card, emblem and prestige." },
    ["KILLSTREAKS"] = { "SCORESTREAKS", "Pick the streaks you earn in a match." },
    ["PERSONALIZATION"] = { "PERSONALIZATION", "Your calling card, emblem and faction." },
    ["CALLING CARD & EMBLEM"] = { "CALLING CARD & EMBLEM", "Your calling card, emblem and faction." },
    ["SCORESTREAKS"] = { "SCORESTREAKS", "Pick the streaks you earn in a match." },
    ["CUSTOM CALLING CARDS"] = { "CUSTOM CALLING CARDS", "Our own calling cards, animated, on your banner." },
    ["CUSTOM EMBLEMS"] = { "CUSTOM EMBLEMS", "Our own emblems, beside your banner." },
    ["START GAME"] = { "START MATCH", "Everyone in, the match begins." },
    ["START MATCH"] = { "START MATCH", "Everyone in, the match begins." },
    ["GAME SETUP"] = { "GAME SETTINGS", "The map, the mode, the bots and the rules." },
    ["CHANGE MAP"] = { "CHANGE MAP", "Every map from every game installed." },
    ["MAP"] = { "MAP", "Every map from every game installed." },
    ["GAME MODE"] = { "GAME MODE", "Team Deathmatch, Domination, Search & Destroy..." },
    ["COMBAT RECORD"] = { "COMBAT RECORD", "Your kills, wins and time played." },
    ["PRESTIGE"] = { "PRESTIGE", "Reach the top level to enter the next prestige." },
    ["CHALLENGES"] = { "CHALLENGES", "Challenges and the camos and cards they unlock." },
    ["LEADERBOARDS"] = { "LEADERBOARDS", "Where you stand against everyone else." },
}

local build_cac_panel -- (defined with the banner, further down)
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

-- A row that carries a value as well as its label (MODE: Team Deathmatch,
-- BOT DIFFICULTY: Regular, ...): the label at the row's left, the value at
-- its right, as view.rs lays them out (SpaceBetween). Right-aligning both
-- put them on top of each other.
local function split_row(texts)
    local ts = CoD.TextSettings.BodyFont or CoD.TextSettings.TitleFontTiny
    local h = UI.px(30)
    local label = nil
    for _, el in ipairs(texts) do
        if not label and tostring(el.id or "") == "text_label" then
            label = el
        end
    end
    label = label or texts[1]
    local function put(el, is_label)
        local function st(c, a)
            local s = {
                alignment = is_label and LUI.Alignment.Left or LUI.Alignment.Right,
                font = ts.Font,
                topAnchor = false,
                bottomAnchor = false,
                leftAnchor = is_label,
                rightAnchor = not is_label,
                top = -h / 2,
                bottom = h / 2,
                width = is_label and UI.px(260) or UI.px(220),
                red = c.r,
                green = c.g,
                blue = c.b,
                alpha = a,
            }
            if is_label then
                s.left = UI.px(36)
            else
                s.right = -UI.px(36)
            end
            return s
        end
        el:registerAnimationState("default", st(is_label and UI.TEXT or UI.DIM, is_label and 0.82 or 0.9))
        el:registerAnimationState("focus", st(UI.WHITE, 1))
        el:animateToState("default")
    end
    for _, el in ipairs(texts) do
        put(el, el == label)
    end
end

local function row_words(menu)
    if not menu.list or menu.claude_words then
        return
    end
    menu.claude_words = true
    local body = (CoD.TextSettings.BodyFont or CoD.TextSettings.TitleFontTiny).Font
    -- The line under the list: an accent bar and the words about the row.
    local line = LUI.UIElement.new({
        topAnchor = false, bottomAnchor = true, leftAnchor = true, rightAnchor = true,
        bottom = UI.px(52), height = UI.px(26), left = 0, right = 0,
    })
    line.claude = true
    local mark = LUI.UIImage.new({
        topAnchor = true, bottomAnchor = true, leftAnchor = true, rightAnchor = false,
        top = 2, bottom = -2, left = 0, width = math.max(3, UI.px(6)),
        material = UI.material("white"), color = UI.ACCENT, alpha = 0,
    })
    local words = LUI.UIText.new({
        topAnchor = true, bottomAnchor = true, leftAnchor = true, rightAnchor = true,
        top = 0, bottom = 0, left = UI.px(18), right = 0,
        font = body, alignment = LUI.Alignment.Left, color = UI.TEXT, alpha = 1,
    })
    words:setText("")
    line:addElement(mark)
    line:addElement(words)
    -- (Under the list, placed from where the list is now; the menu, not
    -- the list, holds it so it is never one of the rows.)
    local ok = pcall(function()
        local lx, _, _, lb = menu.list:getCurrentGlobalRect()
        local mx, my = menu:getCurrentGlobalRect()
        line:registerAnimationState("default", {
            topAnchor = true, bottomAnchor = false, leftAnchor = true, rightAnchor = false,
            left = lx - mx, top = lb - my + UI.px(20), width = UI.px(760), height = UI.px(26),
        })
        line:animateToState("default")
        menu:addElement(line)
    end)
    if not ok then
        return
    end
    local function show(text)
        pcall(function()
            words:setText(text or "")
            mark:setAlpha((text and text ~= "") and 1 or 0)
        end)
    end
    -- (Killstreaks and Personalization are not rows of their own in our
    -- shell: streaks are picked in Create-a-Class, the card and emblem in
    -- Barracks. Their actions are kept for those screens.)
    local moved = {}
    local c0 = menu.list:getFirstChild()
    while c0 do
        local lbl = nil
        walk(c0, 0, function(el)
            if not lbl and el.id == "text_label" then
                lbl = el
            end
        end)
        if lbl then
            local t = ""
            pcall(function()
                t = string.upper(lbl:getText() or "")
            end)
            log("row: '" .. t .. "' action=" .. tostring(c0.properties and c0.properties.button_action_func ~= nil) .. " in " .. tostring(menu.id))
            if (t == "KILLSTREAKS" or t == "PERSONALIZATION") and c0.properties and c0.properties.button_action_func then
                UI.actions = UI.actions or {}
                UI.actions[t] = c0.properties.button_action_func
                moved[#moved + 1] = c0
            end
        end
        c0 = c0:getNextSibling()
    end
    for _, b in ipairs(moved) do
        pcall(function()
            b:close()
        end)
    end

    local c = menu.list:getFirstChild()
    while c do
        local btn = c
        local label = nil
        walk(btn, 0, function(el)
            if not label and el.id == "text_label" then
                label = el
            end
        end)
        if label then
            local have = ""
            pcall(function()
                have = string.upper(label:getText() or "")
            end)
            local w = ROW_WORDS[have]
            if w then
                pcall(function()
                    label:setText(w[1])
                end)
                local spy = LUI.UIElement.new({
                    topAnchor = true, leftAnchor = true, width = 1, height = 1,
                })
                spy.claude = true
                spy:registerEventHandler("button_over", function()
                    show(w[2])
                end)
                spy:registerEventHandler("button_over_disable", function()
                    show(w[2])
                end)
                pcall(function()
                    btn:addElement(spy)
                end)
            end
        end
        c = c:getNextSibling()
    end
end

-- Rows our shell has on a screen that HMW's lacks (their actions are the
-- moved rows' own).
local function add_moved_rows(menu, key)
    local acts = UI.actions or {}
    if not menu.AddButton or menu.claude_added then
        return
    end
    menu.claude_added = true
    local function add(label, act)
        if act then
            pcall(function()
                menu:AddButton(label, function(el, ev)
                    act(el, ev)
                end)
            end)
        end
    end
    if key == "CacMenu_container" then
        add("SCORESTREAKS", acts.KILLSTREAKS)
    elseif key == "OperatorMenu_container" then
        add("CALLING CARD & EMBLEM", acts.PERSONALIZATION)
        local function open(name)
            return function(el, ev)
                LUI.FlowManager.RequestAddMenu(el, name, true, ev and ev.controller or 0, false)
            end
        end
    end
end

local function after_build(menu)
    local key = tostring(menu.id or menu.type or "?")
    local count = 0
    pcall(add_moved_rows, menu, key)
    pcall(row_words, menu)
    if key == "CacMenu_container" and not menu.claude_cac then
        menu.claude_cac = true
        local ok, err = pcall(build_cac_panel, menu)
        if not ok then
            log("cac panel: " .. tostring(err))
        end
    end
    -- The texts of each list row, so a row with a value as well as its label
    -- is laid out as a pair, not both pushed to the right end.
    local per_row = {}
    if menu.list then
        pcall(function()
            walk(menu.list, 0, function(el)
                if el.claude or type(el.setText) ~= "function" or type(el.getText) ~= "function" then
                    return
                end
                local shown = ""
                pcall(function()
                    shown = tostring(el:getText() or "")
                end)
                if shown == "" then
                    return
                end
                local row = el
                while row:getParent() and row:getParent() ~= menu.list do
                    row = row:getParent()
                end
                per_row[row] = per_row[row] or {}
                per_row[row][#per_row[row] + 1] = el
            end)
        end)
    end
    local split_done = {}
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
                local row = el
                while row:getParent() and row:getParent() ~= menu.list do
                    row = row:getParent()
                end
                local texts = per_row[row]
                if texts and #texts > 1 then
                    if not split_done[row] then
                        split_done[row] = true
                        local ok, err = pcall(split_row, texts)
                        if ok then
                            count = count + 1
                        else
                            log("split row: " .. tostring(err))
                        end
                    end
                else
                    local ok, err = pcall(realign_label, el)
                    if ok then
                        count = count + 1
                    else
                        log("realign: " .. tostring(err))
                    end
                end
            end
        end
    end)
    -- The title: our bar and one line of text (the trail). The title piece
    -- can be the same one from screen to screen, so it is pointed at the
    -- screen now up each time, and its line worked out again.
    pcall(function()
        local title = menu:getChildById("generic_title")
        if not title then
            return
        end
        local function find(root, id)
            local hit = nil
            walk(root, 0, function(el)
                if not hit and el.id == id then
                    hit = el
                end
            end)
            return hit
        end
        local function text_of(el)
            local ok, t = pcall(function()
                return el:getText()
            end)
            return ok and type(t) == "string" and t or ""
        end
        local crumb = find(title, "breadcrumb_text")
        local head = find(title, "header_text")
        title.claude_menu_id = tostring(menu.id)
        title.claude_ticks = 0
        local function line_now()
            local ours = SCREEN_TITLES[title.claude_menu_id or ""]
            if ours then
                return ours
            end
            local l = text_of(head)
            local c = text_of(crumb)
            if c ~= "" and l ~= "" then
                return c .. "  /  " .. l
            elseif l == "" then
                return c
            end
            return l
        end
        if not title.claude_bar then
            title.claude_bar = true
            -- (Coordinates inside generic_title, whose corner sits at about
            -- 101, 43 on a 1080-line screen.)
            local bar = LUI.UIImage.new({
                topAnchor = true, bottomAnchor = false, leftAnchor = true, rightAnchor = false,
                top = UI.px(45), height = UI.px(40), left = UI.px(-1), width = math.max(3, UI.px(7)),
                material = UI.material("white"),
                red = UI.ACCENT.r, green = UI.ACCENT.g, blue = UI.ACCENT.b, alpha = 1,
            })
            bar.claude = true
            title:addElement(bar)
            local font = (CoD.TextSettings.BodyFont or CoD.TextSettings.TitleFontTiny).Font
            local t = LUI.UIText.new({
                topAnchor = true, bottomAnchor = false, leftAnchor = true, rightAnchor = false,
                top = UI.px(41), height = UI.px(48), left = UI.px(21), width = UI.px(1400),
                alignment = LUI.Alignment.Left, font = font,
                red = 1, green = 1, blue = 1, alpha = 1,
            })
            t.claude = true
            title:addElement(t)
            title.claude_text = t
            -- (The game types its title in: kept in step for a few seconds
            -- after each screen comes up.)
            title:addElement(LUI.UITimer.new(150, "claude_title_tick"))
            title:registerEventHandler("claude_title_tick", function()
                title.claude_ticks = (title.claude_ticks or 0) + 1
                if title.claude_ticks > 40 then
                    return
                end
                pcall(function()
                    local l2 = line_now()
                    if l2 ~= "" then
                        title.claude_text:setText(l2)
                    end
                    if crumb then crumb:setAlpha(0) end
                    if head then head:setAlpha(0) end
                end)
            end)
        end
        pcall(function()
            title.claude_text:setText(line_now())
            if crumb then crumb:setAlpha(0) end
            if head then head:setAlpha(0) end
        end)
        log("title: " .. tostring(menu.id) .. " -> '" .. line_now() .. "'")
    end)
    if not logged_trees[key] then
        logged_trees[key] = true
        log("menu '" .. key .. "': realigned " .. count .. " row labels")
        local lines = 0
        -- (Create-a-Class: the whole tree, to rebuild it as our shell's.)
        local deep = string.find(key, "Cac") ~= nil
        local max_lines, max_depth = deep and 600 or 60, deep and 9 or 4
        walk(menu, 0, function(el, depth)
            if lines < max_lines and depth <= max_depth then
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
    -- Create-a-Class: only the title, the classes, the hints and ours stay.
    if tostring(menu.id) == "CacMenu_container" then
        local c = menu:getFirstChild()
        local after = false
        while c do
            local id = tostring(c.id or "")
            if after and not c.claude and c ~= menu.list and not string.find(id, "help")
                and not string.find(id, "LUITimer") and id ~= "CacMenu_list" then
                hide(c)
            end
            if id == "generic_title" then
                after = true
            end
            c = c:getNextSibling()
        end
    end
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

-- HMW's player card is never built: our banner takes its place.
if in_frontend and type(AddPlayerCard) == "function" then
    AddPlayerCard = function(menu, controller)
        pcall(function()
            menu.claude_card_slot = true
        end)
    end
end

-- Killstreaks and Personalization are not rows of the lobby screens in our
-- shell (streaks are in Create-a-Class, the card and emblem in Barracks):
-- HMW adds both from this one function.
if in_frontend and LUI.MPLobbyBase and type(LUI.MPLobbyBase.AddPersonalizationButton) == "function" then
    LUI.MPLobbyBase.AddPersonalizationButton = function(menu) end
    UI.actions = UI.actions or {}
    UI.actions.KILLSTREAKS = function(el, ev)
        LUI.FlowManager.RequestAddMenu(nil, "killstreak_selection", true, Cac.GetSelectedControllerIndex(), nil, {
            storageCategory = "Primary",
            weaponType = "streak",
            menuTitle = "@LUA_MENU_KILLSTREAKS",
            showAttributes = false,
            subCategories = {},
            attributesDelta = false,
            optionsForVL = {},
            showUnlocks = true,
        })
    end
    UI.actions.PERSONALIZATION = function(el, ev)
        LUI.FlowManager.RequestAddMenu(el, "PlayercardMenu", true, (ev and ev.controller) or 0, false)
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

-- Our custom cards and emblems in a picker of their own (as our shell's
-- CUSTOM category): a tile for each, with its first frame and its name, the
-- one in use marked; a click puts it on the banner (clc_card / clc_emblem).
-- Opened from Barracks.
local CUSTOM_TITLES = {
    card_above_the_clouds = "Above the Clouds", card_freedom = "Freedom", card_awakened_eye = "Awakened Eye",
    card_the_founder = "The Founder", card_dread = "Dread", card_the_rumbling = "The Rumbling",
    card_third_impact = "Third Impact", card_frieren_himmel = "The Promise", card_himmel = "Himmel",
    card_stark = "Stark", card_campfire = "Campfire", card_quiet_forest = "Quiet Forest",
    card_frieren = "Frieren", card_kittens_look_up = "Stripe", card_sleepy_paws = "Sleepy Paws",
    card_snuggle = "Snuggle", card_mural = "Mural", emblem_legacy = "Legacy", emblem_berserk = "Berserk",
}

local function custom_picker(kind)
    local cards = kind == "cards"
    local list = cards and CUSTOM_CARDS or CUSTOM_EMBLEMS
    local dvar = cards and "clc_card" or "clc_emblem"
    return function(a1, a2, ...)
        local menu = LUI.MenuTemplate.new(a1, {
            menu_title = cards and "MULTIPLAYER  /  BARRACKS  /  CUSTOM CALLING CARDS" or "MULTIPLAYER  /  BARRACKS  /  CUSTOM EMBLEMS",
        })
        local body = (CoD.TextSettings.BodyFont or CoD.TextSettings.TitleFontTiny).Font
        local cols = cards and 3 or 5
        local tw, th = cards and 400 or 170, cards and 100 or 170
        local gx, gy = cards and 20 or 24, cards and 44 or 52
        local x0, y0 = 150, 230
        local tiles = {}
        local function st(x1, y1, x2, y2)
            return {
                topAnchor = true, leftAnchor = true, bottomAnchor = false, rightAnchor = false,
                left = UI.px(x1), top = UI.px(y1), width = UI.px(x2 - x1), height = UI.px(y2 - y1),
            }
        end
        local entries = { { "", 1 } }
        for _, e in ipairs(list) do
            entries[#entries + 1] = e
        end
        local function current()
            local v = ""
            pcall(function()
                v = Engine.GetDvarString(dvar) or ""
            end)
            return v
        end
        local function refresh()
            local cur = current()
            for _, t in ipairs(tiles) do
                pcall(function()
                    t.mark:setAlpha(t.name == cur and 1 or 0)
                end)
            end
        end
        for i, e in ipairs(entries) do
            local c = (i - 1) % cols
            local r = math.floor((i - 1) / cols)
            local x = x0 + c * (tw + gx)
            local y = y0 + r * (th + gy)
            local holder = LUI.UIElement.new(st(x, y, x + tw, y + th + 30))
            holder.claude = true
            local bg = st(0, 0, tw, th)
            bg.material = UI.material("white")
            bg.color = UI.PANEL
            bg.alpha = 0.85
            holder:addElement(LUI.UIImage.new(bg))
            if e[1] ~= "" then
                local im = st(0, 0, tw, th)
                im.material = custom_frame(kind, e, 0) or UI.material("white")
                im.alpha = custom_frame(kind, e, 0) and 1 or 0
                holder:addElement(LUI.UIImage.new(im))
            end
            local mk = st(0, th - 4, tw, th)
            mk.material = UI.material("white")
            mk.color = UI.ACCENT
            mk.alpha = 0
            local mark = LUI.UIImage.new(mk)
            holder:addElement(mark)
            local tx = st(0, th + 4, tw, th + 26)
            tx.font = body
            tx.alignment = LUI.Alignment.Left
            tx.color = UI.TEXT
            local label = LUI.UIText.new(tx)
            label:setText(e[1] == "" and "Game's own" or (CUSTOM_TITLES[e[1]] or e[1]))
            holder:addElement(label)
            holder:setHandleMouseButton(true)
            local name = e[1]
            holder:registerEventHandler("leftmouseup", function()
                pcall(function()
                    Engine.SetDvarFromString(dvar, name)
                end)
                log(dvar .. " = '" .. name .. "'")
                refresh()
            end)
            menu:addElement(holder)
            tiles[#tiles + 1] = { name = name, mark = mark }
        end
        refresh()
        pcall(function()
            menu:AddBackButton()
        end)
        return menu
    end
end

pcall(function()
    LUI.MenuBuilder.m_types_build.claude_custom_cards = custom_picker("cards")
    LUI.MenuBuilder.m_types_build.claude_custom_emblems = custom_picker("emblems")
end)

-- Calling cards and emblems on one screen, as our shell's pickers (cards.rs:
-- one game's pictures at a time, a category list at the left, a grid of
-- pictures with their names, pages): MWR (the game's own tables), MW2R (the
-- same tables' Modern Warfare 2 Remastered entries) and CUSTOM (ours). A
-- category with nothing in it is not listed. Replaces HMW's own Emblem and
-- Calling Card screens; if this fails to build, HMW's is kept.
local function table_items(kind)
    local cards = kind == "cards"
    local out = {}
    local file = cards and CallingCardTable and CallingCardTable.File or (EmblemIconTable and EmblemIconTable.File)
    if not file then
        return out
    end
    local n = 0
    pcall(function()
        n = tonumber(Engine.TableGetRowCount(file)) or 0
    end)
    for row = 0, n - 1 do
        pcall(function()
            local img, id, label
            if cards then
                img = Engine.TableLookupByRow(file, row, CallingCardTable.Cols.Image)
                id = row
                if CallingCardTable.Cols.Name then
                    label = Engine.TableLookupByRow(file, row, CallingCardTable.Cols.Name)
                end
            else
                img = Engine.TableLookupByRow(file, row, EmblemIconTable.Cols.Image)
                id = tonumber(Engine.TableLookupByRow(file, row, EmblemIconTable.Cols.ID))
                if EmblemIconTable.Cols.Name then
                    label = Engine.TableLookupByRow(file, row, EmblemIconTable.Cols.Name)
                end
            end
            if img and img ~= "" and id ~= nil then
                local text = ""
                if label and label ~= "" then
                    pcall(function()
                        text = Engine.Localize(label) or ""
                    end)
                    if text == "" or text == label then
                        text = label
                    end
                end
                if text == "" then
                    text = tostring(img):gsub("^cardtitle_", ""):gsub("^cardicon_", ""):gsub("_", " ")
                end
                out[#out + 1] = { img = tostring(img), id = id, label = text }
            end
        end)
    end
    return out
end

-- Modern Warfare 2 Remastered's entries carry h2 in their picture's name.
local function is_mw2r(img)
    local l = string.lower(img)
    return l:find("^h2_") ~= nil or l:find("_h2_") ~= nil or l:find("^h2") ~= nil
end

local function cards_picker(kind, original)
    local cards = kind == "cards"
    local list = cards and CUSTOM_CARDS or CUSTOM_EMBLEMS
    local dvar = cards and "clc_card" or "clc_emblem"
    local stat_name = cards and "callingCardIndex" or "emblemPatchIndex"
    return function(a1, a2, ...)
        local ok, menu_or_err = pcall(function()
            local menu = LUI.MenuTemplate.new(a1, {
                menu_title = cards and "MULTIPLAYER  /  BARRACKS  /  CALLING CARD" or "MULTIPLAYER  /  BARRACKS  /  EMBLEM",
            })
            local body = (CoD.TextSettings.BodyFont or CoD.TextSettings.TitleFontTiny).Font
            local bold = (CoD.TextSettings.BodyFontBold or CoD.TextSettings.BodyFont or CoD.TextSettings.TitleFontTiny).Font

            -- The three categories, each with its items.
            local mwr, mw2r, custom = {}, {}, {}
            for _, it in ipairs(table_items(kind)) do
                it.kind = "table"
                if is_mw2r(it.img) then
                    mw2r[#mw2r + 1] = it
                else
                    mwr[#mwr + 1] = it
                end
            end
            for _, e in ipairs(list) do
                custom[#custom + 1] = { kind = "custom", entry = e, name = e[1], label = CUSTOM_TITLES[e[1]] or e[1] }
            end
            local cats = {}
            if #mwr > 0 then cats[#cats + 1] = { title = "MWR", items = mwr } end
            if #mw2r > 0 then cats[#cats + 1] = { title = "MW2R", items = mw2r } end
            if #custom > 0 then cats[#cats + 1] = { title = "CUSTOM", items = custom } end
            log(string.format("%s picker: %d MWR, %d MW2R, %d custom", kind, #mwr, #mw2r, #custom))
            if #cats == 0 then
                error("nothing to list")
            end

            local cols = cards and 3 or 6
            local rows = cards and 8 or 4
            local tw, th = cards and 360 or 150, cards and 52 or 150
            local gx, gy = cards and 12 or 14, cards and 10 or 40
            local per_page = cols * rows
            local x0, y0 = 360, 230
            local function st(x1, y1, x2, y2)
                return {
                    topAnchor = true, leftAnchor = true, bottomAnchor = false, rightAnchor = false,
                    left = UI.px(x1), top = UI.px(y1), width = UI.px(x2 - x1), height = UI.px(y2 - y1),
                }
            end
            local function put_text(parent, x1, y1, x2, y2, str, font, color, alpha, align)
                local t = st(x1, y1, x2, y2)
                t.font = font
                t.alignment = align or LUI.Alignment.Left
                t.color = color
                t.alpha = alpha or 1
                local el = LUI.UIText.new(t)
                el:setText(str)
                parent:addElement(el)
                return el
            end

            local state = { cat = 1, page = 1 }
            -- Start on the category his current pick is in.
            do
                local cur = ""
                pcall(function() cur = Engine.GetDvarString(dvar) or "" end)
                if cur ~= "" then
                    for i, c in ipairs(cats) do
                        if c.title == "CUSTOM" then state.cat = i end
                    end
                end
            end

            local function chosen(it)
                local cur = ""
                pcall(function() cur = Engine.GetDvarString(dvar) or "" end)
                if it.kind == "custom" then
                    return cur == it.name
                end
                if cur ~= "" then
                    return false
                end
                local v = nil
                pcall(function()
                    v = tonumber(Engine.GetPlayerData(0, CoD.StatsGroup.Common, stat_name))
                end)
                return v ~= nil and v == it.id
            end

            local grid = LUI.UIElement.new(st(0, 0, 1920, 1080))
            grid.claude = true
            local side = LUI.UIElement.new(st(0, 0, 1920, 1080))
            side.claude = true
            menu:addElement(side)
            menu:addElement(grid)

            local render_side
            local function clear(el)
                local c = el:getFirstChild()
                while c do
                    local nxt = c:getNextSibling()
                    pcall(function() c:close() end)
                    c = nxt
                end
            end

            local function render_grid()
                clear(grid)
                local cat = cats[state.cat]
                local total = #cat.items
                local pages = math.max(1, math.ceil(total / per_page))
                state.page = math.max(1, math.min(state.page, pages))
                local first = (state.page - 1) * per_page
                for i = 1, per_page do
                    local it = cat.items[first + i]
                    if not it then
                        break
                    end
                    local c = (i - 1) % cols
                    local r = math.floor((i - 1) / cols)
                    local x = x0 + c * (tw + gx)
                    local y = y0 + r * (th + gy)
                    local holder = LUI.UIElement.new(st(x, y, x + tw, y + th + (cards and 0 or 28)))
                    holder.claude = true
                    local bg = st(0, 0, tw, th)
                    bg.material = UI.material("white")
                    bg.color = UI.PANEL
                    bg.alpha = 0.85
                    holder:addElement(LUI.UIImage.new(bg))
                    local m = nil
                    if it.kind == "custom" then
                        m = custom_frame(kind, it.entry, 0)
                    else
                        pcall(function() m = UI.material(it.img) end)
                    end
                    if m then
                        local im = st(0, 0, tw, th)
                        im.material = m
                        im.alpha = 1
                        holder:addElement(LUI.UIImage.new(im))
                    end
                    local on = chosen(it)
                    local mk = st(0, th - 4, tw, th)
                    mk.material = UI.material("white")
                    mk.color = UI.ACCENT
                    mk.alpha = on and 1 or 0
                    holder:addElement(LUI.UIImage.new(mk))
                    if cards then
                        -- (Its name over the picture's left, on a shade.)
                        local sh = st(0, 0, tw * 0.55, th)
                        sh.material = UI.material("white")
                        sh.color = UI.BLACK
                        sh.alpha = 0.45
                        holder:addElement(LUI.UIImage.new(sh))
                        put_text(holder, 10, 14, tw * 0.55, th - 8, it.label, bold, UI.WHITE, 1)
                    else
                        put_text(holder, 0, th + 4, tw, th + 26, it.label, body, UI.TEXT, 0.9)
                    end
                    holder:setHandleMouseButton(true)
                    holder:registerEventHandler("leftmouseup", function()
                        pcall(function()
                            if it.kind == "custom" then
                                Engine.SetDvarFromString(dvar, it.name)
                            else
                                Engine.SetDvarFromString(dvar, "")
                                Engine.SetPlayerData(0, CoD.StatsGroup.Common, stat_name, it.id)
                            end
                        end)
                        log(kind .. " picked: " .. tostring(it.name or it.img))
                        render_grid()
                    end)
                    grid:addElement(holder)
                end
                -- Pages: the arrows and "PAGE 1/3" under the grid.
                local py = y0 + rows * (th + gy) + 10
                put_text(grid, x0 + 120, py, x0 + 420, py + 28, "PAGE " .. state.page .. "/" .. pages, body, UI.DIM, 1, LUI.Alignment.Left)
                local function arrow(x, glyph, delta)
                    local el = put_text(grid, x, py, x + 80, py + 30, glyph, bold, UI.WHITE, pages > 1 and 1 or 0.25, LUI.Alignment.Left)
                    pcall(click_area, grid, st(x - 10, py - 4, x + 80, py + 34), function()
                        state.page = state.page + delta
                        if state.page < 1 then state.page = pages end
                        if state.page > pages then state.page = 1 end
                        render_grid()
                    end)
                    return el
                end
                arrow(x0, "<", -1)
                arrow(x0 + 70, ">", 1)
            end

            render_side = function()
                clear(side)
                -- The category list at the left, as our shell's (one game each).
                local ty = y0
                for i, c in ipairs(cats) do
                    local on = i == state.cat
                    if on then
                        local band = st(150, ty, 340, ty + 38)
                        band.material = UI.material("white")
                        band.color = UI.ACCENT
                        band.alpha = 0.88
                        side:addElement(LUI.UIImage.new(band))
                    end
                    put_text(side, 150, ty + 5, 330, ty + 33, c.title, on and bold or body, on and UI.WHITE or UI.TEXT, on and 1 or 0.8, LUI.Alignment.Right)
                    pcall(click_area, side, st(150, ty, 340, ty + 38), function()
                        state.cat = i
                        state.page = 1
                        render_side()
                        render_grid()
                    end)
                    ty = ty + 46
                end
            end

            render_side()
            render_grid()
            pcall(function()
                menu:AddBackButton()
            end)
            return menu
        end)
        if ok then
            return menu_or_err
        end
        log(kind .. " picker failed, HMW's kept: " .. tostring(menu_or_err))
        if original then
            return original(a1, a2, ...)
        end
        error(menu_or_err, 0)
    end
end

pcall(function()
    local types = LUI.MenuBuilder.m_types_build
    for name, kind in pairs({ EmblemMenu = "emblems", CallingCardMenu = "cards" }) do
        if types[name] and not UI["cards_" .. name] then
            UI["cards_" .. name] = true
            types[name] = cards_picker(kind, types[name])
            log("replaced " .. name .. " with our picker")
        else
            log("no " .. name .. " menu type to replace")
        end
    end
end)

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

    -- His numbers over the banner: each number then its word, laid out by
    -- their widths so none runs into the next.
    image(root, 1138, 78, 1162, 98, p.rank_icon, UI.WHITE, p.rank_icon and 1 or 0)
    t.level_small = text(root, 1166, 76, 24, p.level, bold, UI.WHITE, 1, nil, 120)
    t.kills = text(root, 1200, 76, 24, "", bold, UI.WHITE, 1, nil, 120)
    t.kills_w = text(root, 1200, 82, 16, "KILLS", body, UI.DIM, 0.6, nil, 120)
    t.deaths = text(root, 1268, 76, 24, "", bold, UI.WHITE, 1, nil, 120)
    t.deaths_w = text(root, 1268, 82, 16, "DEATHS", body, UI.DIM, 0.6, nil, 120)
    t.kd = text(root, 1345, 76, 24, "", bold, UI.WHITE, 1, nil, 120)
    t.kd_w = text(root, 1392, 82, 16, "K/D RATIO", body, UI.DIM, 0.6, nil, 160)
    local function lay_numbers(q)
        local x = 1166
        local function put(el, str, font, h, gap)
            el:setText(str)
            local w = UI.text_width(str, font, UI.px(h)) / UI.K
            el:setLeftRight(true, false, UI.px(x) - ox, UI.px(x + w + 4) - ox)
            x = x + w + gap
        end
        put(t.level_small, q.level, bold, 24, 20)
        put(t.kills, tostring(q.kills), bold, 24, 6)
        put(t.kills_w, "KILLS", body, 16, 20)
        put(t.deaths, tostring(q.deaths), bold, 24, 6)
        put(t.deaths_w, "DEATHS", body, 16, 20)
        put(t.kd, string.format("%.2f", q.kd), bold, 24, 6)
        put(t.kd_w, "K/D RATIO", body, 16, 20)
    end
    pcall(lay_numbers, p)

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

    -- His line in the party (not on the card / emblem pickers: their grids
    -- reach under it).
    local pickers = { EmblemMenu_container = true, CallingCardMenu_container = true, PlayercardMenu_container = true }
    if not pickers[tostring(menu.id)] then
    text(root, 1176, 236, 20, ">", body, UI.WHITE, 1)
    image(root, 1202, 234, 1226, 256, p.rank_icon, UI.WHITE, p.rank_icon and 1 or 0)
    t.party_level = text(root, 1236, 234, 24, p.level, body, UI.WHITE, 0.9)
    t.party_name = text(root, 1290, 234, 24, p.name, body, UI.WHITE, 0.9)
    image(root, 1178, 261, 1758, 262, UI.material("white"), UI.WHITE, 0.25)
    end

    -- Kept current (a match, a prestige, a new card).
    root:addElement(LUI.UITimer.new(2000, "claude_banner_tick"))
    root:registerEventHandler("claude_banner_tick", function()
        pcall(function()
            local q = player_info()
            lay_numbers(q)
            t.name:setText(string.upper(q.name))
            t.level_big:setText(q.level)
            t.xp:setText("LEVEL " .. q.level .. "  \194\183  " .. tostring(q.xp) .. " XP")
            if t.party_level then
                t.party_level:setText(q.level)
                t.party_name:setText(q.name)
            end
        end)
    end)
    menu:addElement(root)
    return root
end

-- Create-a-Class, as our shell's (cac.rs): the classes at the left, and in
-- the middle the class in focus laid out: PRIMARY and SECONDARY with their
-- pictures and what is on them, LETHAL / TACTICAL, the three PERKS with
-- their coloured rules. HMW's own panels on that screen are hidden; its
-- rows still open its editor.
local function cac_item(cat, ref)
    local name, img = "", nil
    if not ref or ref == "" or ref == "none" or ref == "specialty_null" then
        return "None", nil
    end
    pcall(function()
        name = Cac.GetWeaponName(cat, ref) or ""
    end)
    pcall(function()
        local n = Cac.GetWeaponImageName(cat, ref)
        if n and n ~= "" and n ~= "ui_transparent" then
            img = UI.material(n)
        end
    end)
    if name == "" or string.sub(name, 1, 1) == "@" then
        name = ref
    end
    return name, img
end

function build_cac_panel(menu, fixed_index)
    local ox, oy = menu:getCurrentGlobalRect()
    ox, oy = ox or 0, oy or 0
    local body = (CoD.TextSettings.BodyFont or CoD.TextSettings.TitleFontTiny).Font
    local bold = (CoD.TextSettings.BodyFontBold or CoD.TextSettings.BodyFont).Font
    local function box(x1, y1, x2, y2)
        return {
            topAnchor = true, leftAnchor = true, bottomAnchor = false, rightAnchor = false,
            left = UI.px(x1) - ox, top = UI.px(y1) - oy,
            width = UI.px(x2 - x1), height = UI.px(y2 - y1),
        }
    end
    local root = LUI.UIElement.new({
        topAnchor = true, bottomAnchor = true, leftAnchor = true, rightAnchor = true,
        top = 0, bottom = 0, left = 0, right = 0,
    })
    root.claude = true
    local function image(x1, y1, x2, y2, mat, c, a)
        local st = box(x1, y1, x2, y2)
        st.material = mat or UI.material("white")
        st.color = c or UI.WHITE
        st.alpha = a or 1
        local el = LUI.UIImage.new(st)
        root:addElement(el)
        return el
    end
    local function text(x1, y, h, str, font, c, a, w, align)
        local st = box(x1, y, x1 + (w or 400), y + h)
        st.font = font
        st.alignment = align or LUI.Alignment.Left
        st.color = c or UI.WHITE
        st.alpha = a or 1
        local el = LUI.UIText.new(st)
        el:setText(str)
        root:addElement(el)
        return el
    end
    local function rule(y)
        image(788, y, 1296, y + 1, nil, UI.WHITE, 0.12)
    end
    local function pic(el, mat)
        pcall(function()
            if mat then
                el:setImage(mat)
                el:setAlpha(1)
            else
                el:setAlpha(0)
            end
        end)
    end

    -- (A dark panel behind it, so it reads over the backdrop.)
    image(770, 240, 1314, 790, nil, UI.PANEL, 0.55)
    local w = {}
    text(788, 254, 22, "PRIMARY", body, UI.DIM, 0.75)
    image(866, 256, 867, 276, nil, UI.WHITE, 0.3)
    w.pname = text(876, 252, 26, "", bold, UI.WHITE, 1, 400)
    w.pimg = image(830, 288, 1050, 368, nil, UI.WHITE, 0)
    w.pslots = {}
    for i = 0, 3 do
        local x = 788 + i * 127
        if i > 0 then
            image(x, 384, x + 1, 430, nil, UI.WHITE, 0.12)
        end
        w.pslots[i + 1] = image(x + 43, 386, x + 85, 428, nil, UI.WHITE, 0)
    end
    rule(442)
    text(788, 452, 22, "SECONDARY", body, UI.DIM, 0.75)
    image(894, 454, 895, 474, nil, UI.WHITE, 0.3)
    w.sname = text(904, 450, 26, "", bold, UI.WHITE, 1, 380)
    w.simg = image(830, 486, 960, 540, nil, UI.WHITE, 0)
    w.sslots = { image(1020, 490, 1060, 530, nil, UI.WHITE, 0), image(1100, 490, 1140, 530, nil, UI.WHITE, 0) }
    rule(552)
    text(788, 562, 22, "LETHAL", body, UI.DIM, 0.75)
    text(1052, 562, 22, "TACTICAL", body, UI.DIM, 0.75)
    image(1042, 562, 1043, 636, nil, UI.WHITE, 0.12)
    w.limg = image(798, 596, 830, 628, nil, UI.WHITE, 0)
    w.lname = text(846, 600, 22, "", body, UI.WHITE, 0.9, 190)
    w.timg = image(1062, 596, 1094, 628, nil, UI.WHITE, 0)
    w.tname = text(1110, 600, 22, "", body, UI.WHITE, 0.9, 186)
    rule(648)
    text(788, 658, 22, "PERKS", body, UI.DIM, 0.75)
    w.perks = {}
    local rules = { UI.ENEMY, { r = 0.7, g = 0.7, b = 0.72 }, UI.ACCENT }
    for i = 1, 3 do
        local x = 788 + (i - 1) * 170
        w.perks[i] = {
            img = image(x + 66, 688, x + 104, 726, nil, UI.WHITE, 0),
            name = text(x, 730, 20, "", body, UI.WHITE, 0.9, 166, LUI.Alignment.Center),
        }
        image(x, 756, x + 166, 760, nil, rules[i], 0.95)
    end

    local function render(index)
        local loc = Cac.GetCustomClassLoc()
        local lo = Cac.GetLoadout(loc, index)
        if type(lo) ~= "table" then
            return
        end
        local n, m = cac_item("Primary", lo.primary)
        w.pname:setText(string.upper(n))
        pic(w.pimg, m)
        local pk = { { "Primary_AttachKit", lo.primaryAttachKit }, { "Primary_FurnitureKit", lo.primaryFurnitureKit },
            { "Primary_Reticle", lo.primaryReticle }, { "Primary_Camo", lo.primaryCamo } }
        for i, it in ipairs(pk) do
            local _, im = cac_item(it[1], it[2])
            pic(w.pslots[i], im)
        end
        n, m = cac_item("Secondary", lo.secondary)
        w.sname:setText(string.upper(n))
        pic(w.simg, m)
        local _, a1 = cac_item("Secondary_AttachKit", lo.secondaryAttachKit)
        local _, a2 = cac_item("Secondary_Camo", lo.secondaryCamo)
        pic(w.sslots[1], a1)
        pic(w.sslots[2], a2)
        n, m = cac_item("Lethal", lo.lethal)
        w.lname:setText(n)
        pic(w.limg, m)
        n, m = cac_item("Tactical", lo.tactical)
        w.tname:setText(n)
        pic(w.timg, m)
        for i, cat in ipairs({ "Perk_Slot1", "Perk_Slot2", "Perk_Slot3" }) do
            n, m = cac_item(cat, lo["perk" .. i])
            w.perks[i].name:setText(n)
            pic(w.perks[i].img, m)
        end
    end

    -- The class in focus: each class row says when it is in focus.
    local c = menu.list and menu.list:getFirstChild() or nil
    while c do
        local id = tostring(c.id or "")
        local index = tonumber(string.match(id, "cac_class_button_(%d+)"))
        if index then
            local spy = LUI.UIElement.new({ topAnchor = true, leftAnchor = true, width = 1, height = 1 })
            spy.claude = true
            spy:registerEventHandler("button_over", function()
                local ok, err = pcall(render, index)
                if not ok then
                    log("cac panel: " .. tostring(err))
                end
            end)
            pcall(function()
                c:addElement(spy)
            end)
        end
        c = c:getNextSibling()
    end
    pcall(render, fixed_index or 0)
    menu:addElement(root)
    if fixed_index then
        -- (The editor of one slot: its loadout is shown again as it changes.)
        root:addElement(LUI.UITimer.new(500, "claude_cac_refresh"))
        root:registerEventHandler("claude_cac_refresh", function()
            pcall(render, fixed_index)
        end)
    end
    log("cac panel: built" .. (fixed_index and (" for slot " .. tostring(fixed_index + 1)) or ""))
end

-- The editor of one class (CREATE A CLASS / CUSTOM SLOT n) gets the same
-- panel at the right, as our shell's editor keeps the loadout beside its
-- pickers. Found by its title, whatever the menu is called.
local function probe_cac_editor(menu)
    if menu.claude_cac or menu.claude_cac_editor then
        return
    end
    local title = menu:getChildById("generic_title")
    if not title then
        return
    end
    local found = nil
    walk(title, 0, function(el)
        if not found and el.id == "header_text" then
            found = el
        end
    end)
    if not found then
        return
    end
    local t = ""
    pcall(function()
        t = string.upper(found:getText() or "")
    end)
    local n = tonumber(string.match(t, "CUSTOM%s+SLOT%s+(%d+)"))
        or tonumber(string.match(t, "CUSTOM%s+(%d+)$"))
    if n then
        menu.claude_cac_editor = true
        local ok, err = pcall(build_cac_panel, menu, n - 1)
        if not ok then
            log("cac editor panel: " .. tostring(err))
        end
    end
end

local banner_logged = false
local function place_card(menu)
    -- Our banner where HMW would have put its card (AddPlayerCard below
    -- marks the place instead of building HMW's).
    local wanted = menu.claude_card_slot or (menu.subContainer and menu.subContainer.claude_card_slot)
    -- (A card HMW built anyway, by another way: hidden.)
    for _, bg in ipairs(UI.card_bgs or {}) do
        pcall(function()
            local card = bg:getParent()
            local inside, p = false, card
            while p do
                if p == menu then
                    inside = true
                    break
                end
                p = p:getParent()
            end
            if inside then
                wanted = true
                hide(card)
            end
        end)
    end
    if wanted and not menu.claude_banner then
        local ok, err = pcall(build_banner, menu)
        menu.claude_banner = true
        if not banner_logged then
            banner_logged = true
            log("banner: " .. (ok and "built" or tostring(err)))
        end
    end
end

-- The key hints along the foot (ESC BACK, ENTER SELECT, ...), as our shell's
-- footer draws them: small capitals, the key in the accent and what it does
-- dim. They are found by where they are (the bottom of the screen), not by
-- name, and done again when HMW changes their words.
local HINT_KEYS = { ESC = true, ENTER = true, TAB = true, SPACE = true, DEL = true, DELETE = true }
local function is_key_word(w)
    if HINT_KEYS[w] then
        return true
    end
    -- F1.., single letters, and combinations like Q / E or A / D.
    return string.match(w, "^F%d+$") ~= nil or string.match(w, "^%a$") ~= nil or string.match(w, "^[%a%d]%s*/%s*[%a%d]$") ~= nil
end

local function restyle_footer(menu)
    if not frontend_now() then
        return
    end
    local top_of_foot = UI.px(930)
    walk(menu, 0, function(el)
        if el.claude or type(el.getText) ~= "function" or type(el.getCurrentGlobalRect) ~= "function" then
            return
        end
        local ok, _, y1 = pcall(function()
            return el:getCurrentGlobalRect()
        end)
        if not ok or type(y1) ~= "number" or y1 < top_of_foot then
            return
        end
        local text = ""
        pcall(function()
            text = el:getText() or ""
        end)
        if text == "" then
            return
        end
        local upper = string.upper(text)
        if upper ~= text then
            pcall(function()
                el:setText(upper)
            end)
        end
        local c, a = UI.DIM, 0.85
        if is_key_word(upper) then
            c, a = UI.ACCENT_LIFT, 1
        end
        pcall(function()
            el:setRGB(c.r, c.g, c.b)
        end)
        pcall(function()
            el:setAlpha(a)
        end)
        el.claude_hint = true
    end)
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
        if sweeps <= 60 or sweeps % 5 == 0 then
            pcall(restyle_footer, menu)
        end
    end)
    pcall(function()
        local key = tostring(menu.id or menu.type or "?")
        if backdrop_wanted(key) then
            -- The lobby room is behind our backdrop: put it in its "obscured"
            -- mode so its soldier stops playing (and making sound) unseen.
            -- (Create-a-Class pushes its own mode and gives this one back.)
            -- (Off: forcing the lobby into its "obscured" mode outside the
            -- game's own push/pop is the likeliest cause of the script-string
            -- memory crash after ~12 minutes in the menus. Set
            -- clc_vlobby_obscure 1 to try it again.)
            pcall(function()
                if Engine.GetDvarInt("clc_vlobby_obscure") == 1 then
                    Engine.SetDvarInt("virtualLobbyMode", VirtualLobbyModes.LUI_MODE_OBSCURED)
                end
            end)
            local ok, err = pcall(add_backdrop, menu)
            if not ok then
                log("backdrop: " .. tostring(err))
            end
        end
    end)
    -- (Often: HMW puts its card back whenever the lobby rebuilds it.)
    menu:addElement(LUI.UITimer.new(50, "claude_card_tick"))
    menu:registerEventHandler("claude_card_tick", function()
        local ok, err = pcall(place_card, menu)
        if not ok and not card_logged then
            card_logged = true
            log("card: " .. tostring(err))
        end
    end)
    local probes = 0
    menu:addElement(LUI.UITimer.new(400, "claude_cac_probe"))
    menu:registerEventHandler("claude_cac_probe", function()
        probes = probes + 1
        if probes <= 25 then
            pcall(probe_cac_editor, menu)
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

----------------------------------------------------------------------------
-- 7. Map select: an "AW (2014)" tab for Advanced Warfare's maps in
-- hmw-usermaps (already in this game's zone format, each with its .arena).
-- HMW's map screen has its tabs written into it (MWR / MW2 / MW3), so its
-- AddTabs is replaced by the same one with our tab added; choosing one of
-- ours sets the map and leaves, as HMW's single-map choice does.
----------------------------------------------------------------------------
if in_frontend and LUI.MapSetup and type(LUI.MapSetup.AddTabs) == "function" then
do
    local AW_MAPS = {
        { "mp_refraction", "Ascend" }, { "mp_lab2", "Bio Lab" }, { "mp_comeback", "Comeback" },
        { "mp_laser2", "Defender" }, { "mp_detroit", "Detroit" }, { "mp_greenband", "Greenband" },
        { "mp_levity", "Horizon" }, { "mp_instinct", "Instinct" }, { "mp_recovery", "Recovery" },
        { "mp_venus", "Retreat" }, { "mp_prison", "Riot" }, { "mp_solar", "Solar" },
        { "mp_terrace", "Terrace" }, { "mp_dam", "Atlas Gorge" }, { "mp_clowntown3", "Sideshow" },
        { "mp_urban", "Urban" }, { "mp_climate_3", "Climate" }, { "mp_perplex_1", "Perplex" },
        { "mp_kremlin", "Kremlin" }, { "mp_bigben2", "Parliament" }, { "mp_highrise2", "Skyrise" },
        { "mp_blackbox", "Compound" }, { "mp_fracture", "Fracture" },
        -- (Their names unconfirmed: the map's own file name for now.)
        { "mp_lost", "Lost" }, { "mp_torqued", "Torqued" }, { "mp_spark", "Spark" },
        { "mp_liberty", "Liberty" }, { "mp_lair", "Lair" }, { "mp_seoul2", "Seoul" },
        { "mp_sector17", "Sector 17" },
    }

    local function aw_name(e)
        local ok, n = pcall(function()
            return Engine.Localize("@MPUI_" .. string.upper(string.sub(e[1], 4)))
        end)
        if ok and type(n) == "string" and n ~= "" and not string.find(n, "MPUI_") then
            return n
        end
        return e[2]
    end

    LUI.AWMaps = InheritFrom(LUI.UIElement)
    LUI.AWMaps.Buttons = {}
    LUI.AWMaps.MapFeeder = function(menu, menu2)
        LUI.AWMaps.Buttons = {}
        local current = Engine.GetDvarString("ui_mapname")
        for _, e in ipairs(AW_MAPS) do
            local load_name = e[1]
            local name = aw_name(e)
            local data = {
                button_text = name,
                variant = GenericButtonSettings.Variants.FlatButton,
                content_margin = 0,
                content_width = 60,
            }
            local button = menu2:AddButton(name, function(el, ev)
                Engine.SetDvarString("ui_mapname", load_name)
                pcall(function()
                    Engine.ExecNow("xupdatepartystate")
                end)
                Engine.SetDvarBool("ui_showDLCMaps", false)
                LUI.FlowManager.RequestLeaveMenu(el)
            end, nil, nil, nil, data)
            button.listDefaultFocus = load_name == current
            LUI.AWMaps.Buttons[#LUI.AWMaps.Buttons + 1] = button
        end
    end
    LUI.AWMaps.GetTabLabel = function()
        return "AW (2014)"
    end
    LUI.AWMaps.GetTabReferrer = function()
        return "s1"
    end
    LUI.AWMaps.new = function()
        local st = CoD.CreateState(nil, 80, nil, nil, CoD.AnchorTypes.TopLeft)
        st.alpha = 0
        local root = LUI.UIElement.new(st)
        root.id = "AWMapsRoot"
        root:setClass(LUI.AWMaps)
        root:registerAnimationState("disabled", { alpha = 0 })
        root:registerAnimationState("enabled", { alpha = 1 })
        return root
    end
    LUI.MenuBuilder.m_types_build.s1_tab = LUI.AWMaps.new

    -- Our shell's map picker (view.rs map_picker): a tab for each game, then a
    -- grid of cards, each a picture of the map with its name on a strip under
    -- it, the one in use marked SELECTED, the chosen map's name large at the
    -- bottom right. HMW's own list and tabs are kept (hidden) so a card's
    -- click runs exactly the action HMW's row would.
    local function map_picture(load_name)
        local base = string.gsub(load_name, "^mp_", "")
        local names = {
            "preview_" .. load_name, "loadscreen_" .. load_name,
            "preview_" .. base, "loadscreen_" .. base,
            "loadscreen_mp_" .. base .. "_01",
        }
        for _, n in ipairs(names) do
            local ok, has = pcall(UI.has_material, n)
            if ok and has then
                return UI.material(n), n
            end
        end
        return nil, nil
    end

    -- The map a recorded row's action chooses, found by running it with the
    -- calls it makes held back (it sets ui_mapname and leaves the menu).
    local function learn_load_name(fn)
        local learned = nil
        local saved = {}
        local function hold(tbl, key, repl)
            saved[#saved + 1] = { tbl, key, tbl[key] }
            tbl[key] = repl
        end
        local ok = pcall(function()
            local noop = function() end
            hold(Engine, "SetDvarString", function(name, value)
                if name == "ui_mapname" then
                    learned = value
                end
            end)
            hold(Engine, "SetDvarFromString", noop)
            hold(Engine, "SetDvarBool", noop)
            hold(Engine, "SetDvarInt", noop)
            hold(Engine, "ExecNow", noop)
            hold(LUI.FlowManager, "RequestLeaveMenu", noop)
            hold(LUI.FlowManager, "RequestAddMenu", noop)
            fn({}, { controller = 0 })
        end)
        for i = #saved, 1, -1 do
            local e = saved[i]
            e[1][e[2]] = e[3]
        end
        return learned
    end

    local function capture_tab(tab, menu)
        local out = {}
        local dud = setmetatable({}, { __index = function() return function() end end })
        local fake = setmetatable({}, {
            __index = function(_, k)
                if k == "AddButton" then
                    return function(_, text, fn, ...)
                        out[#out + 1] = { name = text, fn = fn }
                        return dud
                    end
                end
                return menu[k]
            end,
            __newindex = function(_, k, v) end,
        })
        pcall(function()
            tab:MapFeeder(fake)
        end)
        return out
    end

    local function build_map_tiles(menu, tabs, tab_manager)
        local body = (CoD.TextSettings.BodyFont or CoD.TextSettings.TitleFontTiny).Font
        local bold = (CoD.TextSettings.BodyFontBold or CoD.TextSettings.BodyFont or CoD.TextSettings.TitleFontTiny).Font
        local pages = {}
        for _, t in ipairs(tabs) do
            local label = ""
            pcall(function() label = t:GetTabLabel() end)
            local maps = capture_tab(t, menu)
            if #maps > 0 then
                pages[#pages + 1] = { label = label, maps = maps }
            end
        end
        if #pages == 0 then
            error("no maps captured")
        end
        local current = ""
        pcall(function() current = Engine.GetDvarString("ui_mapname") or "" end)
        local found_pictures = 0
        for _, pg in ipairs(pages) do
            for _, m in ipairs(pg.maps) do
                m.load = learn_load_name(m.fn)
                if m.load and m.load ~= "" then
                    m.pic = map_picture(m.load)
                    if m.pic then found_pictures = found_pictures + 1 end
                end
            end
        end
        log(string.format("map tiles: %d tabs, %d pictures found", #pages, found_pictures))

        local function st(x1, y1, x2, y2)
            return {
                topAnchor = true, leftAnchor = true, bottomAnchor = false, rightAnchor = false,
                left = UI.px(x1), top = UI.px(y1), width = UI.px(x2 - x1), height = UI.px(y2 - y1),
            }
        end
        local function put_text(parent, x1, y1, x2, y2, str, font, color, alpha, align)
            local t = st(x1, y1, x2, y2)
            t.font = font
            t.alignment = align or LUI.Alignment.Left
            t.color = color
            t.alpha = alpha or 1
            local el = LUI.UIText.new(t)
            el:setText(str)
            parent:addElement(el)
            return el
        end
        local function put_box(parent, x1, y1, x2, y2, color, alpha)
            local t = st(x1, y1, x2, y2)
            t.material = UI.material("white")
            t.color = color
            t.alpha = alpha
            local el = LUI.UIImage.new(t)
            parent:addElement(el)
            return el
        end
        local function clear(el)
            local c = el:getFirstChild()
            while c do
                local nxt = c:getNextSibling()
                pcall(function() c:close() end)
                c = nxt
            end
        end

        local root = LUI.UIElement.new(st(0, 0, 1920, 1080))
        root.claude = true
        local tabs_el = LUI.UIElement.new(st(0, 0, 1920, 1080))
        tabs_el.claude = true
        local grid = LUI.UIElement.new(st(0, 0, 1920, 1080))
        grid.claude = true
        root:addElement(tabs_el)
        root:addElement(grid)
        menu:addElement(root)

        local state = { page = 1, first = 0 }
        local tw, th, gap = 280, 158, 14
        local cols, rows = 6, 4
        local x0, y0 = 72, 236
        local per = cols * rows

        local render_tabs, render_grid
        render_grid = function()
            clear(grid)
            local pg = pages[state.page]
            local total = #pg.maps
            if state.first >= total then state.first = 0 end
            local last = math.min(total, state.first + per)
            for k = state.first + 1, last do
                local m = pg.maps[k]
                local i = k - state.first - 1
                local c = i % cols
                local r = math.floor(i / cols)
                local x = x0 + c * (tw + gap)
                local y = y0 + r * (th + 4)
                local holder = LUI.UIElement.new(st(x, y, x + tw, y + th))
                holder.claude = true
                put_box(holder, 0, 0, tw, th - 10, UI.PANEL, 0.9)
                if m.pic then
                    local im = st(0, 0, tw, th - 54)
                    im.material = m.pic
                    im.alpha = 1
                    holder:addElement(LUI.UIImage.new(im))
                end
                local chosen = m.load ~= nil and m.load == current
                put_box(holder, 0, th - 54, tw, th - 10, chosen and UI.ACCENT or UI.BLACK, chosen and 0.85 or 0.72)
                put_text(holder, 12, th - 50, tw - 12, th - 14, string.upper(tostring(m.name or "")), bold, UI.WHITE, 1)
                if chosen then
                    put_text(holder, 0, 6, tw - 10, 30, "SELECTED", bold, UI.GOLD, 1, LUI.Alignment.Right)
                end
                holder:setHandleMouseButton(true)
                holder:registerEventHandler("leftmouseup", function(el, ev)
                    pcall(m.fn, el, ev or { controller = 0 })
                end)
                grid:addElement(holder)
            end
            -- "13-24 OF 31" and the way on, under the grid.
            local py = y0 + rows * (th + 4) + 6
            if total > per then
                put_text(grid, x0, py, x0 + 520, py + 28,
                    string.format("%d-%d OF %d     CLICK HERE FOR MORE", state.first + 1, last, total), body, UI.DIM, 1)
                pcall(click_area, grid, st(x0, py - 4, x0 + 520, py + 32), function()
                    state.first = state.first + per
                    if state.first >= total then state.first = 0 end
                    render_grid()
                end)
            end
            -- The chosen map's name, large, at the bottom right, its game under.
            local chosen_name, chosen_tab = nil, nil
            for _, pgx in ipairs(pages) do
                for _, mx in ipairs(pgx.maps) do
                    if mx.load ~= nil and mx.load == current then
                        chosen_name, chosen_tab = mx.name, pgx.label
                    end
                end
            end
            if chosen_name then
                put_text(grid, 1000, 905, 1848, 975, string.upper(tostring(chosen_name)), bold, UI.WHITE, 1, LUI.Alignment.Right)
                put_text(grid, 1000, 975, 1848, 1005, string.upper(tostring(chosen_tab or "")), body, UI.ACCENT, 1, LUI.Alignment.Right)
            end
        end

        render_tabs = function()
            clear(tabs_el)
            local x = x0
            for i, pg in ipairs(pages) do
                local on = i == state.page
                local label = string.upper(pg.label)
                local w = math.max(150, UI.text_width(label, bold, UI.px(26)) / UI.K + 44)
                if on then
                    put_box(tabs_el, x, 188, x + w, 226, UI.ACCENT, 0.9)
                end
                put_text(tabs_el, x, 192, x + w, 224, label, on and bold or body, on and UI.WHITE or UI.TEXT, on and 1 or 0.8, LUI.Alignment.Center)
                local idx = i
                pcall(click_area, tabs_el, st(x, 188, x + w, 226), function()
                    state.page = idx
                    state.first = 0
                    render_tabs()
                    render_grid()
                end)
                x = x + w + 8
            end
            put_box(tabs_el, x0, 228, 1848, 230, UI.ACCENT, 0.45)
        end

        -- Start on the tab holding the map in use.
        for i, pg in ipairs(pages) do
            for _, m in ipairs(pg.maps) do
                if m.load ~= nil and m.load == current then
                    state.page = i
                end
            end
        end
        render_tabs()
        render_grid()

        -- HMW's list and tabs stay (their actions are ours), unseen. Its list
        -- is filled after this runs, so it is hidden again for a while.
        local n = 0
        root:addElement(LUI.UITimer.new(150, "claude_maps_hide"))
        root:registerEventHandler("claude_maps_hide", function()
            n = n + 1
            if n <= 40 then
                pcall(hide, menu.list)
                pcall(hide, tab_manager)
            end
        end)
    end

    LUI.MapSetup.AddTabs = function(menu)
        local tabManager = LUI.MenuBuilder.BuildRegisteredType("MFTabManager", {
            defState = CoD.CreateState(nil, nil, nil, nil, CoD.AnchorTypes.TopLeftRight),
            numOfTabs = 4,
        })
        tabManager:keepRightBumperAlignedToHeader(false)
        tabManager.tabSelected = 1
        local tabs = {}
        for _, kind in ipairs({ "h1_tab", "iw4_tab", "iw5_tab", "s1_tab" }) do
            local t = LUI.MenuBuilder.BuildRegisteredType(kind, { controller = menu.controller })
            tabs[#tabs + 1] = t
            menu:addElement(t)
        end
        tabs[1]:MapFeeder(menu)
        for i = 1, #tabs do
            local n = i
            tabManager:addTab(menu.controller, tabs[n]:GetTabLabel(), function()
                menu:ChangeTab(tabs[n], tabManager)
            end)
        end
        tabManager:refreshTab(menu.controller)
        menu:addElement(tabManager)
        local ok, err = pcall(build_map_tiles, menu, tabs, tabManager)
        if not ok then
            log("map tiles: " .. tostring(err))
        end
    end
    log("map select: AW (2014) tab added (" .. #AW_MAPS .. " maps)")
end
end
