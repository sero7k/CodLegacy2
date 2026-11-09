-- Adds every stock MWR (h1_*) weapon to the CAC lists if the HMW list lacks it.
if game:issingleplayer() or (not Engine.InFrontend()) then
    return
end

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
                    if not have[name] then
                        table.insert(list, { name, 0 })
                        added = added + 1
                    end
                end
            end
        end
    end
end)
pcall(function() game:claudelog("mwr_weapons: added " .. added .. " missing stock weapons") end)
