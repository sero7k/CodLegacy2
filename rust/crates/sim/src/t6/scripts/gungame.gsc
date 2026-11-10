// bo2mp: Gun Game's ladder, made to the weapons the catalog has.
//
// maps/mp/gametypes/gun lists each level's gun by the name Black Ops II's
// engine knows it by (`judge_mp+steadyaim`). Its promotion compares the
// weapon a kill was made with to that name, and its takeoldweapons keeps only
// the gun of that name. Our catalog names a gun as it holds it: a variant it
// lacks is its base gun or sight variant, so a kill made with it never
// matched the ladder's name and nobody moved up. Each level's names are
// replaced here by the names the catalog gives them (a level none of whose
// names the catalog has is left out and logged), before the game type starts.

fixladder()
{
    if ( level.gametype != "gun" || !isdefined( level.gunprogression ) )
        return;
    ladder = [];
    foreach ( gun in level.gunprogression )
    {
        names = [];
        foreach ( name in gun.names )
        {
            known = bo2mp_resolveweapon( name );
            if ( known == "" )
                continue;
            have = 0;
            foreach ( other in names )
            {
                if ( other == known )
                    have = 1;
            }
            if ( !have )
                names[names.size] = known;
        }
        if ( names.size == 0 )
        {
            println( "bo2mp gun game: level skipped, no weapon of " + gun.names[0] + " in the catalog" );
            continue;
        }
        gun.names = names;
        ladder[ladder.size] = gun;
    }
    if ( ladder.size == 0 )
    {
        println( "bo2mp gun game: the catalog has none of the ladder's weapons; the ladder is left as the game type made it" );
        return;
    }
    level.gunprogression = ladder;
}
