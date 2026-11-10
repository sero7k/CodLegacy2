<!-- Generated: cargo run -p gsc_iw6_host --example iw6_script_census -- <zone dir> mp_frag --md . Status is read from the host's own dispatcher. -->

# Ghosts script builtin census: mp_frag (313 scripts): reachable from the map's main (1198 functions)

327 builtin ids called. Functions: 80 implemented, 29 approximate, 32 no-op, 0 unimplemented. Methods: 93 implemented, 21 approximate, 72 no-op, 0 unimplemented.

| id | kind | name | conf | status | call sites | scripts |
|---|---|---|---|---|---|---|
| 0x002d | function | isdefined | V | implemented | 1905 | #1209, #1351, #1354, #1355, ... |
| 0x01a6 | function | getnextarraykey | V | implemented | 330 | #1351, #1355, #1365, #1369, ... |
| 0x01c1 | function | getfirstarraykey | V | implemented | 330 | #1351, #1355, #1365, #1369, ... |
| 0x82f1 | method | setclientomnvar | L | implemented | 132 | #1385 maps/mp/_utility, #1465, #1468, #1471, ... |
| 0x003b | function | gettime | L | implemented | 98 | #1355, #1365, #1366, #1385 maps/mp/_utility, ... |
| 0x0113 | function | issubstr | V | implemented | 85 | #1365, #1374, #1385 maps/mp/_utility, #1468, ... |
| 0x00b0 | function | int | V | implemented | 84 | #1355, #1385 maps/mp/_utility, #1468, #1476, ... |
| 0x0189 | function | getentarray | V | implemented | 76 | #1351, #1357 maps/mp/_compass, #1358, #1361, ... |
| 0x01a8 | function | tablelookup | V | implemented | 68 | #1355, #1374, #1385 maps/mp/_utility, #1468, ... |
| 0x019a | function | isplayer | V | implemented | 62 | #1374, #1377, #1385 maps/mp/_utility, #1470, ... |
| 0x80a9 | method | delete | V | implemented | 59 | #1361, #1373 maps/mp/_load, #1466, #1500, ... |
| 0x0143 | function | loadfx | V | implemented | 58 | #1361, #1369, #1373 maps/mp/_load, #1377, ... |
| 0x821c | method | getrankedplayerdata | V | approximate | 52 | #1374, #1385 maps/mp/_utility, #1468, #1476, ... |
| 0x018c | function | spawnstruct | V | implemented | 51 | #1354, #1373 maps/mp/_load, #1385 maps/mp/_utility, #1465, ... |
| 0x823e | method | setcommonplayerdata | V | approximate | 46 | #1355, #1381, #1470, #1476, ... |
| 0x0188 | function | getent | V | implemented | 45 | #1373 maps/mp/_load, #1474 maps/mp/_movers, #920, #921, ... |
| 0x8097 | method | playsound | V | implemented | 40 | #1385 maps/mp/_utility, #1470, #1474 maps/mp/_movers, #1500, ... |
| 0x8269 | method | allowspectateteam | L | approximate | 39 | #1492, #1507 |
| 0x0037 | function | getdvar | V | implemented | 38 | #1357 maps/mp/_compass, #1361, #1373 maps/mp/_load, #1385 maps/mp/_utility, ... |
| 0x0063 | function | spawn | V | implemented | 37 | #1373 maps/mp/_load, #1382, #1466, #1571, ... |
| 0x0122 | function | setmatchdata | V | approximate | 37 | #1374, #1476, #1492 |
| 0x823b | method | setrankedplayerdata | V | approximate | 33 | #1476, #1489, #1494, #1498, ... |
| 0x0032 | function | setdvar | V | implemented | 31 | #1373 maps/mp/_load, #1385 maps/mp/_utility, #1476, #1510, ... |
| 0x0038 | function | getdvarint | V | implemented | 30 | #1374, #1385 maps/mp/_utility, #1492, #1494, ... |
| 0x80aa | method | setmodel | V | implemented | 30 | #1508, #1571, #919 common_scripts/_destructible, #920, ... |
| 0x81ac | method | getentitynumber | V | implemented | 30 | #1358, #1465, #1470, #1488, ... |
| 0x00c3 | function | isai | L | implemented | 29 | #1381, #1468, #1470, #1492, ... |
| 0x0129 | function | setclientmatchdata | V | approximate | 29 | #1370, #1381, #1476 |
| 0x01e9 | function | isagent | L | implemented | 25 | #1374, #1379, #1385 maps/mp/_utility, #1466, ... |
| 0x00d5 | function | max | V | implemented | 23 | #1385 maps/mp/_utility, #1474 maps/mp/_movers, #1500, #1501, ... |
| 0x0130 | function | getmatchrulesdata | V | approximate | 22 | #1468, #1548 |
| 0x8051 | method | hide | L | implemented | 21 | #1373 maps/mp/_load, #1474 maps/mp/_movers, #919 common_scripts/_destructible, #920, ... |
| 0x00e1 | function | distance | V | implemented | 20 | #1365, #1474 maps/mp/_movers, #1500, #916 common_scripts/_createfx, ... |
| 0x823f | method | getcacplayerdata | V | approximate | 20 | #1468, #1548 |
| 0x82a3 | method | moveto | V | implemented | 20 | #1474 maps/mp/_movers, #920, #921, maps/mp/mp_frag |
| 0x82ea | method | playlocalsound | V | implemented | 20 | #1385 maps/mp/_utility, #1465, #1470, #1471, ... |
| 0x82f2 | method | setclientdvar | V | implemented | 20 | #1385 maps/mp/_utility, #1476, #1488, #1500, ... |
| 0x00e9 | function | vectordot | V | implemented | 19 | #1357 maps/mp/_compass, #1470, #1474 maps/mp/_movers, #1506, ... |
| 0x804a | method | linkto | V | implemented | 19 | #1382, #1474 maps/mp/_movers, #1571, #916 common_scripts/_createfx, ... |
| 0x81da | method | setlightintensity | L | implemented | 19 | #919 common_scripts/_destructible, #920, #921 |
| 0x00a9 | function | randomfloatrange | V | implemented | 18 | #1369, #1500, #919 common_scripts/_destructible, #920, ... |
| 0x0107 | function | vectornormalize | V | implemented | 18 | #1470, #1474 maps/mp/_movers, #1506, #916 common_scripts/_createfx, ... |
| 0x00a6 | function | randomint | V | implemented | 17 | #1351, #1382, #1476, #919 common_scripts/_destructible, ... |
| 0x0117 | function | strtok | V | approximate | 17 | #1361, #1385 maps/mp/_utility, #1474 maps/mp/_movers, #1498, ... |
| 0x015e | function | precachemodel | V | implemented | 17 | #1351, #1373 maps/mp/_load, maps/mp/mp_frag |
| 0x8073 | method | playloopsound | V | implemented | 17 | #1380, #1474 maps/mp/_movers, #919 common_scripts/_destructible, #920, ... |
| 0x00b3 | function | min | V | implemented | 16 | #1385 maps/mp/_utility, #1476, #1500, #1501, ... |
| 0x018e | function | isalive | L | implemented | 16 | #1355, #1365, #1385 maps/mp/_utility, #1465, ... |
| 0x80a4 | method | stoploopsound | V | implemented | 16 | #1380, #1474 maps/mp/_movers, #919 common_scripts/_destructible, #920, ... |
| 0x00e3 | function | distancesquared | V | implemented | 15 | #1365, #1466, #1470, #1500, ... |
| 0x00b2 | function | abs | V | implemented | 14 | #1382, #919 common_scripts/_destructible, #920, #921 |
| 0x0253 | function | unk_0253 | G | no-op | 14 | #1468, #1470, #1476, #1478, ... |
| 0x82b2 | method | rotateto | V | implemented | 14 | #1474 maps/mp/_movers, #920, #922 common_scripts/_exploder, maps/mp/mp_frag |
| 0x817b | method | gettagorigin | V | implemented | 13 | #919 common_scripts/_destructible |
| 0x0116 | function | tolower | V | implemented | 12 | #1500, #919 common_scripts/_destructible, #920, #927 common_scripts/utility |
| 0x82b5 | method | setcandamage | L | implemented | 12 | #1361, #919 common_scripts/_destructible, #920, maps/mp/mp_frag |
| 0x00a7 | function | randomfloat | V | implemented | 11 | #1373 maps/mp/_load, #1377, #1382, #919 common_scripts/_destructible, ... |
| 0x010d | function | anglestoforward | V | implemented | 11 | #1470, #1474 maps/mp/_movers, #1506, #916 common_scripts/_createfx, ... |
| 0x8081 | method | fadeovertime | L | implemented | 11 | #1380, #1385 maps/mp/_utility, #1472, #1488 |
| 0x013a | function | precacherumble | L | implemented | 10 | #919 common_scripts/_destructible, #920 |
| 0x8018 | method | attach | L | implemented | 10 | #1508, #919 common_scripts/_destructible |
| 0x80a2 | method | istouching | V | implemented | 10 | #1377, #919 common_scripts/_destructible, #920, #921, ... |
| 0x821f | method | getcommonplayerdata | V | approximate | 10 | #1355, #1476, #1498, #1548 |
| 0x83fd | method | setcamerapoint | G | implemented | 10 | #1500 |
| 0x83fe | method | unk_83fe | G | no-op | 10 | #1476, #1488, #1507 |
| 0x0039 | function | getdvarfloat | V | implemented | 9 | #1373 maps/mp/_load, #1492, #1501, #916 common_scripts/_createfx, ... |
| 0x0144 | function | playfx | V | implemented | 9 | #1361, #916 common_scripts/_createfx, #919 common_scripts/_destructible, #920, ... |
| 0x0145 | function | playfxontag | V | implemented | 9 | #919 common_scripts/_destructible, #920 |
| 0x8050 | method | show | L | implemented | 9 | #1474 maps/mp/_movers, #920, #922 common_scripts/_exploder |
| 0x80c3 | method | setshader | L | implemented | 9 | #1364, #1380, #1472, #1488, ... |
| 0x821a | method | isitemunlocked | V | approximate | 9 | #1365, #1468 |
| 0x8305 | method | getcurrentweapon | V | implemented | 9 | #1365, #1494, #1501, #1511, ... |
| 0x0115 | function | getsubstr | V | implemented | 8 | #1385 maps/mp/_utility, #1468, #1470, #1498, ... |
| 0x0196 | function | newhudelem | V | implemented | 8 | #916 common_scripts/_createfx |
| 0x8085 | method | destroy | L | implemented | 8 | #1364, #1472, #1489, #916 common_scripts/_createfx |
| 0x82aa | method | rotateto_shortest | G | implemented | 8 | #1382, #920 |
| 0x82b4 | method | notsolid | V | implemented | 8 | #1373 maps/mp/_load, #1474 maps/mp/_movers, #919 common_scripts/_destructible, #922 common_scripts/_exploder, ... |
| 0x002f | function | isstring | V | implemented | 7 | #1476, #1510, #921, #922 common_scripts/_exploder |
| 0x01c0 | function | getarraykeys | L | implemented | 7 | #1351, #916 common_scripts/_createfx, #923 common_scripts/_fx |
| 0x01e8 | function | isbot | L | implemented | 7 | #1385 maps/mp/_utility, #1468, #1478, #1501 |
| 0x8003 | method | playsoundtoteam | L | implemented | 7 | #1385 maps/mp/_utility, #1466, #1500 |
| 0x8181 | method | setdepthoffield | V | implemented | 7 | #1385 maps/mp/_utility, #1476, #1500 |
| 0x8242 | method | buttonpressed | V | approximate | 7 | #916 common_scripts/_createfx, #917 |
| 0x8291 | method | unk_8291 | G | no-op | 7 | #1476, #1493 |
| 0x00dc | function | clamp | V | implemented | 6 | #1501, #1511, #1548, #916 common_scripts/_createfx, ... |
| 0x0108 | function | vectortoangles | V | implemented | 6 | #1373 maps/mp/_load, #1382, #916 common_scripts/_createfx, #917, ... |
| 0x0150 | function | getteamscore | L | implemented | 6 | #1381, #1476 |
| 0x0187 | function | unk_0187 | G | no-op | 6 | #1476 |
| 0x01ac | function | tablelookupgetrownum | V | implemented | 6 | #1385 maps/mp/_utility, #1488, #1492, #1501 |
| 0x80c1 | method | settext | L | implemented | 6 | #1385 maps/mp/_utility, #1488 |
| 0x82c1 | method | unk_82c1 | G | no-op | 6 | #1385 maps/mp/_utility, #1488, #1500 |
| 0x82d1 | method | notifyonplayercommand | L | approximate | 6 | #1465, #1470, #1492 |
| 0x830c | method | setoffhandsecondaryclass | L | implemented | 6 | #1385 maps/mp/_utility |
| 0x832f | method | getplayerangles | L | approximate | 6 | #1494, #916 common_scripts/_createfx |
| 0x8338 | method | setoffhandprimaryclass | L | implemented | 6 | #1385 maps/mp/_utility |
| 0x0030 | function | setsaveddvar | L | implemented | 5 | #1476 |
| 0x0123 | function | getmatchdata | V | approximate | 5 | #1374, #1476 |
| 0x0136 | function | visionsetnaked | V | implemented | 5 | #1246, #1373 maps/mp/_load, #1476 |
| 0x0158 | function | positionwouldtelefrag | L | implemented | 5 | #1379, #1500, #1505, #1506 |
| 0x01a5 | function | getbaseweaponname | L | approximate | 5 | #1365, #1468, #1511, #919 common_scripts/_destructible |
| 0x01a7 | function | sortbydistance | V | implemented | 5 | #1379, #1474 maps/mp/_movers, #927 common_scripts/utility |
| 0x8026 | method | detach | L | implemented | 5 | #1477, #1508, #919 common_scripts/_destructible |
| 0x817c | method | gettagangles | V | implemented | 5 | #919 common_scripts/_destructible |
| 0x81aa | method | unk_81aa | G | no-op | 5 | #1380 |
| 0x8265 | method | unk_8265 | G | no-op | 5 | #1358, #1500 |
| 0x826e | method | useanimtree | L | implemented | 5 | #1351, #1373 maps/mp/_load, maps/mp/mp_frag |
| 0x8300 | method | getweaponslistall | V | implemented | 5 | #1385 maps/mp/_utility, #1511 |
| 0x005b | function | getweaponattachments | L | approximate | 4 | #1385 maps/mp/_utility, #1492, #1498, #1511 |
| 0x0066 | function | bullettrace | V | approximate | 4 | #1373 maps/mp/_load, #916 common_scripts/_createfx |
| 0x010b | function | anglestoup | V | implemented | 4 | #916 common_scripts/_createfx, #919 common_scripts/_destructible |
| 0x010c | function | anglestoright | V | implemented | 4 | #916 common_scripts/_createfx, #919 common_scripts/_destructible, #920 |
| 0x01d0 | function | weaponclass | L | approximate | 4 | #1385 maps/mp/_utility, #1511 |
| 0x8056 | method | connectpaths | V | implemented | 4 | #1373 maps/mp/_load, #1474 maps/mp/_movers, maps/mp/mp_frag |
| 0x8057 | method | disconnectpaths | V | implemented | 4 | #1373 maps/mp/_load, #1474 maps/mp/_movers, maps/mp/mp_frag |
| 0x8092 | method | getorigin | V | implemented | 4 | #1377, #920 |
| 0x80a1 | method | geteye | L | implemented | 4 | #1500, #1506, #916 common_scripts/_createfx |
| 0x80d1 | method | setcursorhint | V | no-op | 4 | #920, #921 |
| 0x80d2 | method | sethintstring | V | implemented | 4 | #1474 maps/mp/_movers, #921 |
| 0x8177 | method | getstance | L | implemented | 4 | #1379, #1474 maps/mp/_movers, #1494, #1505 |
| 0x817d | method | shellshock | L | implemented | 4 | #1380 |
| 0x81d0 | method | radiusdamage | V | implemented | 4 | #919 common_scripts/_destructible |
| 0x82ad | method | rotateroll | V | implemented | 4 | #1382, maps/mp/mp_frag |
| 0x82b3 | method | solid | V | implemented | 4 | #919 common_scripts/_destructible, #922 common_scripts/_exploder, maps/mp/mp_frag |
| 0x82b6 | method | setcanradiusdamage | L | implemented | 4 | maps/mp/mp_frag |
| 0x82dc | method | unk_82dc | G | no-op | 4 | #1385 maps/mp/_utility |
| 0x82ec | method | setweaponammoclip | L | implemented | 4 | #1385 maps/mp/_utility, #1468, #1511, #1571 |
| 0x8306 | method | unk_8306 | G | no-op | 4 | #1470 |
| 0x8335 | method | unk_8335 | G | no-op | 4 | #1470, #1494, #1506 |
| 0x8375 | method | (unnamed) | - | no-op | 4 | #1474 maps/mp/_movers, maps/mp/mp_frag |
| 0x00b1 | function | float | V | implemented | 3 | #1474 maps/mp/_movers, #1511 |
| 0x013e | function | precachestring | V | implemented | 3 | #921 |
| 0x0149 | function | spawnfx | V | approximate | 3 | #922 common_scripts/_exploder, #923 common_scripts/_fx, maps/mp/mp_frag |
| 0x014a | function | triggerfx | V | approximate | 3 | #922 common_scripts/_exploder, #923 common_scripts/_fx, maps/mp/mp_frag |
| 0x015f | function | map_restart | L | implemented | 3 | #1476 |
| 0x0176 | function | print3d | G | no-op | 3 | #1377, #916 common_scripts/_createfx, #922 common_scripts/_exploder |
| 0x01a9 | function | tablelookupbyrow | V | implemented | 3 | #1488, #1489 |
| 0x01b7 | function | isweaponvalid | G | approximate | 3 | #1466, #927 common_scripts/utility |
| 0x804c | method | unlink | V | implemented | 3 | #1379, #919 common_scripts/_destructible, maps/mp/mp_frag |
| 0x8072 | method | playsoundasmaster | V | approximate | 3 | #920, #927 common_scripts/utility |
| 0x8082 | method | scaleovertime | L | implemented | 3 | #1488, #1489 |
| 0x8086 | method | setpulsefx | L | implemented | 3 | #1488 |
| 0x80ce | method | setvalue | L | implemented | 3 | #1488, #916 common_scripts/_createfx |
| 0x80d7 | method | (unnamed) | - | no-op | 3 | #1351, #923 common_scripts/_fx, #927 common_scripts/utility |
| 0x82b7 | method | physicslaunchserver | L | no-op | 3 | #919 common_scripts/_destructible, #920, #922 common_scripts/_exploder |
| 0x82de | method | unk_82de | G | no-op | 3 | #1466, #1506 |
| 0x82ed | method | setweaponammostock | L | implemented | 3 | #1468, #1511, #1571 |
| 0x82ee | method | getweaponammoclip | L | implemented | 3 | #1511, #1571 |
| 0x8325 | method | givemaxammo | L | implemented | 3 | #1385 maps/mp/_utility, #1571 |
| 0x832c | method | unk_832c | G | no-op | 3 | #1379 |
| 0x8337 | method | unk_8337 | G | no-op | 3 | #1508 |
| 0x83a7 | method | unk_83a7 | G | no-op | 3 | #1508 |
| 0x83fa | method | unk_83fa | G | no-op | 3 | #1374 |
| 0x8406 | method | unk_8406 | G | no-op | 3 | #1385 maps/mp/_utility, #1466 |
| 0x841d | method | unk_841d | G | no-op | 3 | #1501 |
| 0x0061 | function | unk_0061 | G | no-op | 2 | #1470 |
| 0x0089 | function | getnorthyaw | L | approximate | 2 | #1357 maps/mp/_compass |
| 0x00a5 | function | setslowmotion | L | implemented | 2 | #1492 |
| 0x00b7 | function | getnode | V | approximate | 2 | maps/mp/mp_frag |
| 0x012a | function | getclientmatchdata | L | approximate | 2 | #1370, #1381 |
| 0x013f | function | precachemenu | V | implemented | 2 | #916 common_scripts/_createfx, #921 |
| 0x0159 | function | unk_0159 | G | no-op | 2 | #1379, #1505 |
| 0x0160 | function | exitlevel | L | implemented | 2 | #1476, #1501 |
| 0x0179 | function | earthquake | V | implemented | 2 | #919 common_scripts/_destructible, #927 common_scripts/utility |
| 0x0195 | function | playsoundatposition | L | implemented | 2 | #1474 maps/mp/_movers, maps/mp/mp_frag |
| 0x0197 | function | newclienthudelem | V | implemented | 2 | #1380, #1489 |
| 0x0198 | function | newteamhudelem | L | implemented | 2 | #1364, #1472 |
| 0x021e | function | unk_021e | G | no-op | 2 | #1470, #1506 |
| 0x0220 | function | soundgetplaybacktime | G | approximate | 2 | #1474 maps/mp/_movers |
| 0x8015 | method | unk_8015 | G | no-op | 2 | #1470, #1506 |
| 0x8016 | method | unk_8016 | G | no-op | 2 | #1470 |
| 0x801f | method | unk_801f | G | no-op | 2 | #1470 |
| 0x80cf | method | unk_80cf | G | no-op | 2 | #1364, #1472 |
| 0x8178 | method | setstance | L | approximate | 2 | #1379, #1500 |
| 0x81d9 | method | getlightintensity | L | implemented | 2 | #920 |
| 0x81dc | method | setmovespeedscale | L | implemented | 2 | #1358, #1511 |
| 0x8263 | method | iprintln | G | implemented | 2 | #1501 |
| 0x826d | method | clonebrushmodeltoscriptmodel | V | no-op | 2 | maps/mp/mp_frag |
| 0x8270 | method | scriptmodelplayanim | L | implemented | 2 | maps/mp/mp_frag |
| 0x8293 | method | unk_8293 | G | no-op | 2 | #1500 |
| 0x8296 | method | unk_8296 | G | no-op | 2 | #1501 |
| 0x829b | method | setperk | V | implemented | 2 | #1385 maps/mp/_utility |
| 0x829e | method | unsetperk | V | implemented | 2 | #1385 maps/mp/_utility, #1547 |
| 0x82ae | method | devaddyaw | L | no-op | 2 | #916 common_scripts/_createfx |
| 0x82af | method | devaddroll | L | no-op | 2 | #916 common_scripts/_createfx |
| 0x82b0 | method | devaddpitch | L | no-op | 2 | #916 common_scripts/_createfx |
| 0x82bd | method | unk_82bd | G | no-op | 2 | #1500 |
| 0x82ef | method | getweaponammostock | L | implemented | 2 | #1511, #1571 |
| 0x82f3 | method | setclientdvars | L | implemented | 2 | #1476 |
| 0x8302 | method | giveweapon | V | implemented | 2 | #1385 maps/mp/_utility |
| 0x8303 | method | takeweapon | V | implemented | 2 | #1548 |
| 0x8316 | method | openpopupmenu | L | implemented | 2 | #916 common_scripts/_createfx, #921 |
| 0x831c | method | unk_831c | G | implemented | 2 | #1385 maps/mp/_utility |
| 0x833d | method | setscriptmoverkillcam | L | implemented | 2 | #1373 maps/mp/_load |
| 0x8352 | method | unk_8352 | G | no-op | 2 | #1470 |
| 0x83e8 | method | unk_83e8 | G | no-op | 2 | #1492 |
| 0x83f1 | method | unk_83f1 | G | no-op | 2 | #1500 |
| 0x83fb | method | unk_83fb | G | no-op | 2 | #1374 |
| 0x8404 | method | playsoundonmovingent | L | implemented | 2 | #1474 maps/mp/_movers |
| 0x840c | method | unk_840c | G | no-op | 2 | #1494, #1511 |
| 0x841c | method | setcommonplayerdatareservedint | V | approximate | 2 | #1381 |
| 0x0034 | function | setdvarifuninitialized | V | implemented | 1 | #916 common_scripts/_createfx |
| 0x005a | function | getweaponmodel | G | approximate | 1 | #1492 |
| 0x0060 | function | unk_0060 | G | no-op | 1 | #1470 |
| 0x0062 | function | unk_0062 | G | no-op | 1 | #1470 |
| 0x0065 | function | (unnamed) | - | no-op | 1 | #927 common_scripts/utility |
| 0x0082 | function | sighttracepassed | L | approximate | 1 | #1500 |
| 0x0083 | function | bullettracepassed | L | approximate | 1 | #1506 |
| 0x0085 | function | playerphysicstrace | G | approximate | 1 | #1500 |
| 0x00a8 | function | randomintrange | V | implemented | 1 | #1385 maps/mp/_utility |
| 0x00aa | function | sin | V | implemented | 1 | #1357 maps/mp/_compass |
| 0x00ab | function | cos | V | implemented | 1 | #1357 maps/mp/_compass |
| 0x00b8 | function | getnodearray | V | approximate | 1 | #1474 maps/mp/_movers |
| 0x00ba | function | unk_00ba | G | no-op | 1 | #1379 |
| 0x00bb | function | unk_00bb | G | no-op | 1 | #1506 |
| 0x00d7 | function | ceil | V | implemented | 1 | #1500 |
| 0x00e0 | function | pointonsegmentnearesttopoint | L | implemented | 1 | #920 |
| 0x00ec | function | visionsetthermal | L | implemented | 1 | #1373 maps/mp/_load |
| 0x00ed | function | visionsetpain | L | implemented | 1 | #1373 maps/mp/_load |
| 0x00ee | function | unk_00ee | G | no-op | 1 | #1476 |
| 0x00f1 | function | bullettracer | G | no-op | 1 | #1506 |
| 0x0118 | function | strcmp | G | implemented | 1 | #927 common_scripts/utility |
| 0x0124 | function | sendmatchdata | L | no-op | 1 | #1476 |
| 0x012b | function | setclientmatchdatadef | L | no-op | 1 | #1476 |
| 0x012c | function | sendclientmatchdata | L | no-op | 1 | #1476 |
| 0x012f | function | unk_012f | G | no-op | 1 | #1501 |
| 0x0132 | function | kick | L | no-op | 1 | #1358 |
| 0x0134 | function | unk_0134 | G | no-op | 1 | #1505 |
| 0x0135 | function | unk_0135 | G | no-op | 1 | #1476 |
| 0x0137 | function | visionsetnight | V | implemented | 1 | #1373 maps/mp/_load |
| 0x0138 | function | visionsetmissilecam | V | implemented | 1 | #1373 maps/mp/_load |
| 0x013c | function | precacheitem | V | implemented | 1 | #1373 maps/mp/_load |
| 0x013d | function | precacheshader | V | implemented | 1 | #916 common_scripts/_createfx |
| 0x0148 | function | playloopedfx | V | approximate | 1 | #923 common_scripts/_fx |
| 0x014c | function | unk_014c | G | no-op | 1 | #1476 |
| 0x0151 | function | setteamscore | L | implemented | 1 | #1478 |
| 0x0157 | function | unk_0157 | G | no-op | 1 | #1470 |
| 0x0169 | function | unk_0169 | G | no-op | 1 | #1476 |
| 0x0170 | function | physicsexplosionsphere | L | implemented | 1 | #919 common_scripts/_destructible |
| 0x0175 | function | (unnamed) | - | no-op | 1 | maps/mp/mp_frag |
| 0x018a | function | getspawnarray | V | approximate | 1 | #1505 |
| 0x018f | function | isvehicle | G | approximate | 1 | #919 common_scripts/_destructible |
| 0x019e | function | weaponclipsize | L | approximate | 1 | #1511 |
| 0x01a4 | function | weaponinventorytype | G | approximate | 1 | #1511 |
| 0x01aa | function | tablelookupistring | L | approximate | 1 | #1501 |
| 0x01be | function | setminimap | L | implemented | 1 | #1357 maps/mp/_compass |
| 0x01d7 | function | getvehiclenode | V | approximate | 1 | #927 common_scripts/utility |
| 0x01e0 | function | botsconnecttype | G | approximate | 1 | #1385 maps/mp/_utility |
| 0x0200 | function | connectnodepair | V | no-op | 1 | maps/mp/mp_frag |
| 0x0201 | function | disconnectnodepair | V | no-op | 1 | maps/mp/mp_frag |
| 0x0212 | function | activateclientexploder | L | implemented | 1 | #922 common_scripts/_exploder |
| 0x023a | function | unk_023a | G | no-op | 1 | #1476 |
| 0x023b | function | unk_023b | G | no-op | 1 | #1476 |
| 0x0243 | function | playsoundtoplayers | G | implemented | 1 | #1385 maps/mp/_utility |
| 0x0248 | function | unk_0248 | G | no-op | 1 | #1571 |
| 0x024f | function | unk_024f | G | no-op | 1 | #1476 |
| 0x0250 | function | unk_0250 | G | no-op | 1 | #1476 |
| 0x0251 | function | unk_0251 | G | no-op | 1 | #1476 |
| 0x8005 | method | unk_8005 | G | no-op | 1 | #1470 |
| 0x800a | method | unk_800a | G | implemented | 1 | #1385 maps/mp/_utility |
| 0x8017 | method | unk_8017 | G | no-op | 1 | #1470 |
| 0x8027 | method | detach | L | implemented | 1 | #1385 maps/mp/_utility |
| 0x8029 | method | unk_8029 | G | no-op | 1 | #1468 |
| 0x8045 | method | hidepart | L | implemented | 1 | #919 common_scripts/_destructible |
| 0x8048 | method | showpart | L | implemented | 1 | #919 common_scripts/_destructible |
| 0x804e | method | viewkick | G | implemented | 1 | #1379 |
| 0x8068 | method | enablelinkto | L | implemented | 1 | #1474 maps/mp/_movers |
| 0x8083 | method | moveovertime | L | implemented | 1 | #1489 |
| 0x808f | method | unk_808f | G | no-op | 1 | #1379 |
| 0x80a6 | method | unk_80a6 | G | no-op | 1 | #922 common_scripts/_exploder |
| 0x80a8 | method | unk_80a8 | G | no-op | 1 | #1500 |
| 0x80ab | method | unk_80ab | G | no-op | 1 | #1373 maps/mp/_load |
| 0x80ac | method | unk_80ac | G | no-op | 1 | #1373 maps/mp/_load |
| 0x80b1 | method | unk_80b1 | G | no-op | 1 | #1470 |
| 0x80b7 | method | setcontents | L | approximate | 1 | #1571 |
| 0x80b8 | method | makeusable | L | implemented | 1 | #1474 maps/mp/_movers |
| 0x80b9 | method | makeunusable | L | implemented | 1 | #1474 maps/mp/_movers |
| 0x80c2 | method | unk_80c2 | G | no-op | 1 | #916 common_scripts/_createfx |
| 0x80c6 | method | settimer | L | implemented | 1 | #1385 maps/mp/_utility |
| 0x8176 | method | unk_8176 | G | no-op | 1 | #920 |
| 0x817f | method | unk_817f | G | no-op | 1 | #1500 |
| 0x81ae | method | unk_81ae | G | no-op | 1 | #920 |
| 0x81db | method | unk_81db | G | no-op | 1 | #1470 |
| 0x821d | method | getprivateplayerdata | V | approximate | 1 | #1508 |
| 0x8245 | method | setspawnweapon | L | implemented | 1 | #1468 |
| 0x8247 | method | unk_8247 | G | no-op | 1 | #1511 |
| 0x8261 | method | suicide | L | implemented | 1 | #1385 maps/mp/_utility |
| 0x8264 | method | iprintlnbold | G | implemented | 1 | #1470 |
| 0x8267 | method | cloneplayer | L | implemented | 1 | #1470 |
| 0x826a | method | getguid | V | approximate | 1 | #1385 maps/mp/_utility |
| 0x826f | method | scriptmodelclearanim | L | implemented | 1 | #1373 maps/mp/_load |
| 0x828c | method | getxuid | L | approximate | 1 | #1476 |
| 0x8295 | method | unk_8295 | G | no-op | 1 | #1478 |
| 0x8299 | method | unk_8299 | G | no-op | 1 | #1492 |
| 0x829d | method | clearperks | V | implemented | 1 | #1385 maps/mp/_utility |
| 0x82a7 | method | movegravity | L | approximate | 1 | #922 common_scripts/_exploder |
| 0x82bf | method | unk_82bf | G | implemented | 1 | #1385 maps/mp/_utility |
| 0x82c0 | method | unk_82c0 | G | no-op | 1 | #1547 |
| 0x82c2 | method | getweaponslistprimaries | L | implemented | 1 | #1511 |
| 0x82c3 | method | getweaponslistoffhands | L | implemented | 1 | #1548 |
| 0x82c8 | method | unk_82c8 | G | no-op | 1 | #1385 maps/mp/_utility |
| 0x82d4 | method | unk_82d4 | G | no-op | 1 | #1470 |
| 0x82d5 | method | unk_82d5 | G | no-op | 1 | #1500 |
| 0x82fc | method | setweaponhudiconoverrides | V | implemented | 1 | #1385 maps/mp/_utility |
| 0x8304 | method | takeallweapons | L | implemented | 1 | #1468 |
| 0x8308 | method | hasweapon | V | implemented | 1 | #1511 |
| 0x8309 | method | unk_8309 | G | no-op | 1 | #1468 |
| 0x8318 | method | closepopupmenu | L | implemented | 1 | #916 common_scripts/_createfx |
| 0x831e | method | unk_831e | G | implemented | 1 | #927 common_scripts/utility |
| 0x8321 | method | unk_8321 | G | no-op | 1 | #1354 |
| 0x8330 | method | unk_8330 | G | no-op | 1 | #1500 |
| 0x8334 | method | playerads | L | approximate | 1 | #1385 maps/mp/_utility |
| 0x833e | method | unk_833e | G | no-op | 1 | #927 common_scripts/utility |
| 0x833f | method | unk_833f | G | no-op | 1 | #1465 |
| 0x8340 | method | unk_8340 | G | no-op | 1 | #1465 |
| 0x836d | method | unk_836d | G | no-op | 1 | #1385 maps/mp/_utility |
| 0x83b7 | method | unk_83b7 | G | no-op | 1 | #1364 |
| 0x83b8 | method | unk_83b8 | G | no-op | 1 | #1571 |
| 0x83d8 | method | unk_83d8 | G | no-op | 1 | #1508 |
| 0x83d9 | method | unk_83d9 | G | no-op | 1 | #1508 |
| 0x83da | method | unk_83da | G | no-op | 1 | #1508 |
| 0x83fc | method | unk_83fc | G | no-op | 1 | #1385 maps/mp/_utility |
| 0x8402 | method | getlinkedchildren | L | implemented | 1 | #1379 |
| 0x8405 | method | unk_8405 | G | no-op | 1 | #1379 |
| 0x840d | method | unk_840d | G | no-op | 1 | #1511 |
| 0x841e | method | getrankedplayerdatareservedint | V | approximate | 1 | #1501 |
| 0x841f | method | setrankedplayerdatareservedint | V | approximate | 1 | #1501 |
| 0x8423 | method | unk_8423 | G | no-op | 1 | #1488 |

# Ghosts script builtin census: mp_frag (313 scripts): every script loaded (animscripts, bots and player code included)

979 builtin ids called. Functions: 89 implemented, 34 approximate, 34 no-op, 176 unimplemented. Methods: 99 implemented, 26 approximate, 83 no-op, 438 unimplemented.

| id | kind | name | conf | status | call sites | scripts |
|---|---|---|---|---|---|---|
| 0x002d | function | isdefined | V | implemented | 11579 | #1000, #1001, #1002, #1003, ... |
| 0x01a6 | function | getnextarraykey | V | implemented | 1578 | #1001, #1002, #1006, #1011, ... |
| 0x01c1 | function | getfirstarraykey | V | implemented | 1578 | #1001, #1002, #1006, #1011, ... |
| 0x003b | function | gettime | L | implemented | 741 | #1002, #1006, #1020, #1036, ... |
| 0x80a9 | method | delete | V | implemented | 527 | #1002, #1005, #1006, #1024, ... |
| 0x00b0 | function | int | V | implemented | 389 | #1001, #1002, #1006, #1023, ... |
| 0x019a | function | isplayer | V | implemented | 381 | #1002, #1005, #1006, #1023, ... |
| 0x82f1 | method | setclientomnvar | L | implemented | 378 | #1365, #1385 maps/mp/_utility, #1465, #1468, ... |
| 0x018c | function | spawnstruct | V | implemented | 349 | #1000, #1002, #1005, #1014, ... |
| 0x0189 | function | getentarray | V | implemented | 333 | #1001, #1002, #1006, #1014, ... |
| 0x00a6 | function | randomint | V | implemented | 328 | #1006, #1011, #1014, #1023, ... |
| 0x0143 | function | loadfx | V | implemented | 321 | #1002, #1024, #1066, #1120, ... |
| 0x0113 | function | issubstr | V | implemented | 308 | #1002, #1020, #1023, #1036, ... |
| 0x018e | function | isalive | L | implemented | 307 | #1005, #1006, #1014, #1036, ... |
| 0x010d | function | anglestoforward | V | implemented | 304 | #1002, #1005, #1006, #1014, ... |
| 0x0063 | function | spawn | V | implemented | 279 | #1002, #1005, #1006, #1020, ... |
| 0x0038 | function | getdvarint | V | implemented | 247 | #1001, #1002, #1061, #1063, ... |
| 0x00e1 | function | distance | V | implemented | 221 | #1001, #1005, #1006, #1020, ... |
| 0x0032 | function | setdvar | V | implemented | 220 | #1002, #1006, #1020, #1023, ... |
| 0x00e3 | function | distancesquared | V | implemented | 210 | #1006, #1014, #1036, #1061, ... |
| 0x80aa | method | setmodel | V | implemented | 209 | #1002, #1006, #1024, #1036, ... |
| 0x01a8 | function | tablelookup | V | implemented | 199 | #1020, #1355, #1374, #1385 maps/mp/_utility, ... |
| 0x8097 | method | playsound | V | implemented | 192 | #1002, #1061, #1063, #1071, ... |
| 0x0037 | function | getdvar | V | implemented | 178 | #1001, #1002, #1006, #1020, ... |
| 0x0145 | function | playfxontag | V | implemented | 178 | #1002, #1006, #1024, #1064, ... |
| 0x00a9 | function | randomfloatrange | V | implemented | 177 | #1001, #1005, #1024, #1036, ... |
| 0x813b | method | (unnamed) | - | UNIMPLEMENTED | 171 | #1002, #1067, #1068, #947, ... |
| 0x00c3 | function | isai | L | implemented | 168 | #1002, #1011, #1036, #1063, ... |
| 0x00a7 | function | randomfloat | V | implemented | 165 | #1001, #1002, #1006, #1036, ... |
| 0x0188 | function | getent | V | implemented | 163 | #1001, #1002, #1006, #1036, ... |
| 0x0033 | function | setdevdvar | G | implemented | 157 | #1385 maps/mp/_utility, maps/mp/gametypes/blitz, maps/mp/gametypes/conf, maps/mp/gametypes/cranked, ... |
| 0x00a8 | function | randomintrange | V | implemented | 153 | #1001, #1014, #1036, #1066, ... |
| 0x0107 | function | vectornormalize | V | implemented | 153 | #1002, #1006, #1014, #1036, ... |
| 0x8051 | method | hide | L | implemented | 153 | #1001, #1006, #1036, #1063, ... |
| 0x817b | method | gettagorigin | V | implemented | 147 | #1006, #1063, #1064, #1066, ... |
| 0x804a | method | linkto | V | implemented | 146 | #1020, #1024, #1063, #1066, ... |
| 0x00e9 | function | vectordot | V | implemented | 136 | #1002, #1006, #1014, #1036, ... |
| 0x00b2 | function | abs | V | implemented | 134 | #1002, #1063, #1067, #1068, ... |
| 0x82ea | method | playlocalsound | V | implemented | 126 | #1002, #1063, #1120, #1371, ... |
| 0x0066 | function | bullettrace | V | approximate | 125 | #1005, #1063, #1068, #1371, ... |
| 0x81ac | method | getentitynumber | V | implemented | 121 | #1358, #1385 maps/mp/_utility, #1465, #1470, ... |
| 0x0144 | function | playfx | V | implemented | 118 | #1063, #1068, #1071, #1361, ... |
| 0x8145 | method | (unnamed) | - | UNIMPLEMENTED | 118 | #1002, animscripts/atv, animscripts/combat, animscripts/combat_utility, ... |
| 0x0108 | function | vectortoangles | V | implemented | 117 | #1005, #1006, #1014, #1036, ... |
| 0x0116 | function | tolower | V | implemented | 112 | #1002, #1006, #1020, #1023, ... |
| 0x818a | method | (unnamed) | - | UNIMPLEMENTED | 112 | #1014, #1063, animscripts/animmode, animscripts/combat, ... |
| 0x82d1 | method | notifyonplayercommand | L | approximate | 112 | #1387, #1465, #1470, #1491, ... |
| 0x821c | method | getrankedplayerdata | V | approximate | 107 | #1374, #1385 maps/mp/_utility, #1468, #1476, ... |
| 0x0130 | function | getmatchrulesdata | V | approximate | 100 | #1385 maps/mp/_utility, #1468, #1548, #1571, ... |
| 0x0122 | function | setmatchdata | V | approximate | 97 | #1374, #1476, #1492, #1500, ... |
| 0x8081 | method | fadeovertime | L | implemented | 97 | #1000, #1002, #1020, #1063, ... |
| 0x00d5 | function | max | V | implemented | 95 | #1001, #1061, #1063, #1068, ... |
| 0x8305 | method | getcurrentweapon | V | implemented | 93 | #1002, #1023, #1036, #1063, ... |
| 0x823e | method | setcommonplayerdata | V | approximate | 89 | #1355, #1374, #1381, #1465, ... |
| 0x82a3 | method | moveto | V | implemented | 85 | #1005, #1063, #1067, #1359, ... |
| 0x00de | function | (unnamed) | - | UNIMPLEMENTED | 83 | #1379, #1491, #1494, #947, ... |
| 0x8278 | method | (unnamed) | - | UNIMPLEMENTED | 81 | #1067, #1068, #1120, #1375, ... |
| 0x8264 | method | iprintlnbold | G | implemented | 80 | #1365, #1385 maps/mp/_utility, #1470, #1493, ... |
| 0x010c | function | anglestoright | V | implemented | 77 | #1014, #1063, #1064, #1067, ... |
| 0x80a2 | method | istouching | V | implemented | 77 | #1002, #1036, #1061, #1063, ... |
| 0x0117 | function | strtok | V | approximate | 74 | #1001, #1006, #1011, #1014, ... |
| 0x8085 | method | destroy | L | implemented | 73 | #1000, #1002, #1063, #1064, ... |
| 0x8144 | method | (unnamed) | - | UNIMPLEMENTED | 73 | #1002, #1067, #1068, #947, ... |
| 0x80a1 | method | geteye | L | implemented | 72 | #1005, #1020, #1036, #1061, ... |
| 0x8106 | method | (unnamed) | - | UNIMPLEMENTED | 72 | #1014, animscripts/atv, animscripts/combat, animscripts/combat_utility, ... |
| 0x80c3 | method | setshader | L | implemented | 71 | #1000, #1002, #1020, #1063, ... |
| 0x00b3 | function | min | V | implemented | 67 | #1001, #1006, #1061, #1385 maps/mp/_utility, ... |
| 0x00cd | function | (unnamed) | - | UNIMPLEMENTED | 66 | #1002, #1006, #1011, #1036, ... |
| 0x8050 | method | show | L | implemented | 65 | #1001, #1006, #1036, #1063, ... |
| 0x0039 | function | getdvarfloat | V | implemented | 63 | #1001, #1061, #1063, #1373 maps/mp/_load, ... |
| 0x8251 | method | (unnamed) | - | UNIMPLEMENTED | 63 | #1068, #1120, #1375, #1519, ... |
| 0x0196 | function | newhudelem | V | implemented | 62 | #1000, #1002, #1020, #1063, ... |
| 0x82b5 | method | setcandamage | L | implemented | 62 | #1067, #1068, #1361, #1470, ... |
| 0x01c0 | function | getarraykeys | L | implemented | 61 | #1002, #1006, #1014, #1036, ... |
| 0x01e9 | function | isagent | L | implemented | 60 | #1365, #1374, #1379, #1385 maps/mp/_utility, ... |
| 0x804c | method | unlink | V | implemented | 59 | #1024, #1063, #1066, #1067, ... |
| 0x8188 | method | (unnamed) | - | UNIMPLEMENTED | 59 | animscripts/traverse/crawl, animscripts/traverse/crouch_jump_down_40, animscripts/traverse/duck_under_56, animscripts/traverse/iw6_jumpup_72, ... |
| 0x00dc | function | clamp | V | implemented | 58 | #1001, #1061, #1063, #1068, ... |
| 0x8189 | method | (unnamed) | - | UNIMPLEMENTED | 58 | #1014, animscripts/animmode, animscripts/combat, animscripts/corner, ... |
| 0x00ca | function | (unnamed) | - | UNIMPLEMENTED | 57 | #1001, #1002, #1020, #1061, ... |
| 0x8073 | method | playloopsound | V | implemented | 53 | #1063, #1367, #1375, #1380, ... |
| 0x8303 | method | takeweapon | V | implemented | 53 | #1385 maps/mp/_utility, #1470, #1477, #1483, ... |
| 0x813d | method | (unnamed) | - | UNIMPLEMENTED | 52 | #947, animscripts/atv, animscripts/combat_utility, animscripts/corner, ... |
| 0x817c | method | gettagangles | V | implemented | 51 | #1006, #1063, #1064, #1067, ... |
| 0x80c1 | method | settext | L | implemented | 50 | #1002, #1020, #1063, #1064, ... |
| 0x0083 | function | bullettracepassed | L | approximate | 49 | #1006, #1061, #1063, #1120, ... |
| 0x0060 | function | unk_0060 | G | no-op | 48 | #1002, #1067, #1068, #1470, ... |
| 0x0146 | function | stopfxontag | L | implemented | 48 | #1024, #1068, #1120, #1511, ... |
| 0x8177 | method | getstance | L | implemented | 48 | #1006, #1355, #1379, #1385 maps/mp/_utility, ... |
| 0x0030 | function | setsaveddvar | L | implemented | 47 | #1476, #1480, #1501, #1553, ... |
| 0x010b | function | anglestoup | V | implemented | 47 | #1063, #1064, #1067, #1068, ... |
| 0x823b | method | setrankedplayerdata | V | approximate | 47 | #1374, #1385 maps/mp/_utility, #1476, #1489, ... |
| 0x80a4 | method | stoploopsound | V | implemented | 46 | #1063, #1380, #1474 maps/mp/_movers, #1509, ... |
| 0x8064 | method | (unnamed) | - | UNIMPLEMENTED | 45 | #1006, #1036, #1063, #1068, ... |
| 0x01a5 | function | getbaseweaponname | L | approximate | 44 | #1063, #1365, #1385 maps/mp/_utility, #1466, ... |
| 0x81da | method | setlightintensity | L | implemented | 44 | #1001, #919 common_scripts/_destructible, #920, #921 |
| 0x82ec | method | setweaponammoclip | L | implemented | 44 | #1002, #1004, #1063, #1385 maps/mp/_utility, ... |
| 0x00c2 | function | (unnamed) | - | UNIMPLEMENTED | 43 | #1002, #1063, #1064, #1068, ... |
| 0x00e2 | function | (unnamed) | - | UNIMPLEMENTED | 43 | #1491, #1511, #1519, #1520, ... |
| 0x013d | function | precacheshader | V | implemented | 41 | #1002, #1066, #1364, #1365, ... |
| 0x82ee | method | getweaponammoclip | L | implemented | 41 | #1002, #1004, #1063, #1385 maps/mp/_utility, ... |
| 0x82f2 | method | setclientdvar | V | implemented | 41 | #1385 maps/mp/_utility, #1470, #1476, #1486, ... |
| 0x01cd | function | (unnamed) | - | UNIMPLEMENTED | 40 | #1477, #1507, #1519, #1524, ... |
| 0x014a | function | triggerfx | V | approximate | 39 | #1483, #1503, #1532, #1537, ... |
| 0x01a9 | function | tablelookupbyrow | V | implemented | 39 | #1365, #1385 maps/mp/_utility, #1488, #1489, ... |
| 0x8269 | method | allowspectateteam | L | approximate | 39 | #1492, #1507 |
| 0x82ed | method | setweaponammostock | L | implemented | 39 | #1002, #1004, #1063, #1385 maps/mp/_utility, ... |
| 0x8026 | method | detach | L | implemented | 38 | #1006, #1063, #1067, #1477, ... |
| 0x832f | method | getplayerangles | L | approximate | 38 | #1002, #1061, #1063, #1120, ... |
| 0x0061 | function | unk_0061 | G | no-op | 37 | #1014, #1470, #947, animscripts/animmode, ... |
| 0x0179 | function | earthquake | V | implemented | 37 | #1063, #1068, #1120, #1367, ... |
| 0x8018 | method | attach | L | implemented | 37 | #1006, #1036, #1063, #1067, ... |
| 0x00b1 | function | float | V | implemented | 36 | #1001, #1365, #1379, #1470, ... |
| 0x8211 | method | getpointinbounds | L | implemented | 36 | #1067, #1371, #1372, #1383, ... |
| 0x0087 | function | (unnamed) | - | UNIMPLEMENTED | 35 | #1067, animscripts/combat, animscripts/death, animscripts/grenade_cower, ... |
| 0x00ab | function | cos | V | implemented | 35 | #1063, #1120, #1357 maps/mp/_compass, #1385 maps/mp/_utility, ... |
| 0x0115 | function | getsubstr | V | implemented | 35 | #1002, #1063, #1385 maps/mp/_utility, #1468, ... |
| 0x0253 | function | unk_0253 | G | no-op | 35 | #1374, #1468, #1470, #1476, ... |
| 0x8094 | method | (unnamed) | - | UNIMPLEMENTED | 35 | #967, animscripts/atv, animscripts/combat, animscripts/combat_utility, ... |
| 0x013a | function | precacherumble | L | implemented | 34 | #1002, #1036, #1064, #1066, ... |
| 0x014b | function | playfxontagforclients | L | approximate | 34 | #1002, #1511, #1516, #1523, ... |
| 0x8287 | method | (unnamed) | - | UNIMPLEMENTED | 34 | #1068, #1120, #1375, #1519, ... |
| 0x8309 | method | unk_8309 | G | no-op | 34 | #1002, #1004, #1063, #1385 maps/mp/_utility, ... |
| 0x019e | function | weaponclipsize | L | approximate | 33 | #1002, #1063, #1484, #1485, ... |
| 0x8335 | method | unk_8335 | G | no-op | 32 | #1385 maps/mp/_utility, #1470, #1477, #1491, ... |
| 0x0176 | function | print3d | G | no-op | 31 | #1061, #1377, #1470, #1520, ... |
| 0x01af | function | (unnamed) | - | UNIMPLEMENTED | 31 | #1372, #1512, #1514, #1515, ... |
| 0x8242 | method | buttonpressed | V | approximate | 31 | #1002, #1359, #916 common_scripts/_createfx, #917, ... |
| 0x0197 | function | newclienthudelem | V | implemented | 30 | #1120, #1364, #1380, #1489, ... |
| 0x8006 | method | (unnamed) | - | UNIMPLEMENTED | 30 | #1511, #1519, #1524, #1554, ... |
| 0x80d2 | method | sethintstring | V | implemented | 30 | #1002, #1036, #1379, #1470, ... |
| 0x81d4 | method | (unnamed) | - | UNIMPLEMENTED | 30 | #1372, #1512, #1515, #1516, ... |
| 0x821a | method | isitemunlocked | V | approximate | 30 | #1365, #1366, #1468, #1494, ... |
| 0x0129 | function | setclientmatchdata | V | approximate | 29 | #1370, #1381, #1476 |
| 0x81b9 | method | (unnamed) | - | UNIMPLEMENTED | 29 | #1005, #1006, #1036, animscripts/battlechatter, ... |
| 0x823f | method | getcacplayerdata | V | approximate | 29 | #1468, #1499, #1548, #1571 |
| 0x83b8 | method | unk_83b8 | G | no-op | 29 | #1379, #1477, #1490, #1511, ... |
| 0x00b7 | function | getnode | V | approximate | 28 | #1002, #1006, #1014, #1036, ... |
| 0x00db | function | (unnamed) | - | UNIMPLEMENTED | 28 | #1014, #1063, #1355, #947, ... |
| 0x01b7 | function | isweaponvalid | G | approximate | 28 | #1006, #1061, #1466, #927 common_scripts/utility, ... |
| 0x01cc | function | (unnamed) | - | UNIMPLEMENTED | 28 | #1063, #1477, #1519, #1524, ... |
| 0x81be | method | (unnamed) | - | UNIMPLEMENTED | 28 | #1014, animscripts/combat, animscripts/corner, animscripts/cover_arrival, ... |
| 0x8330 | method | unk_8330 | G | no-op | 28 | #1470, #1477, #1483, #1494, ... |
| 0x00aa | function | sin | V | implemented | 27 | #1001, #1036, #1063, #1357 maps/mp/_compass, ... |
| 0x01ca | function | (unnamed) | - | UNIMPLEMENTED | 27 | #1477, #1507, #1519, #1524, ... |
| 0x01d7 | function | getvehiclenode | V | approximate | 27 | #1063, #1066, #1068, #1565, ... |
| 0x82eb | method | (unnamed) | - | UNIMPLEMENTED | 27 | #1371, #1372, #1495, #1516, ... |
| 0x8300 | method | getweaponslistall | V | implemented | 27 | #1004, #1063, #1385 maps/mp/_utility, #1470, ... |
| 0x00e6 | function | (unnamed) | - | UNIMPLEMENTED | 26 | #1014, #1379, #1470, #1494, ... |
| 0x80b9 | method | makeunusable | L | implemented | 26 | #1002, #1006, #1063, #1068, ... |
| 0x8105 | method | (unnamed) | - | UNIMPLEMENTED | 26 | animscripts/civilian, animscripts/combat, animscripts/combat_utility, animscripts/corner, ... |
| 0x817d | method | shellshock | L | implemented | 26 | #1063, #1068, #1359, #1363, ... |
| 0x0034 | function | setdvarifuninitialized | V | implemented | 25 | #1063, #1476, #1553, #916 common_scripts/_createfx, ... |
| 0x01a7 | function | sortbydistance | V | implemented | 25 | #1002, #1355, #1371, #1372, ... |
| 0x8198 | method | (unnamed) | - | UNIMPLEMENTED | 25 | animscripts/cover_arrival, animscripts/swim, animscripts/traverse/crawl, animscripts/traverse/crouch_jump_down_40, ... |
| 0x82aa | method | rotateto_shortest | G | implemented | 25 | #1063, #1359, #1379, #1382, ... |
| 0x82ef | method | getweaponammostock | L | implemented | 25 | #1002, #1004, #1063, #1385 maps/mp/_utility, ... |
| 0x0195 | function | playsoundatposition | L | implemented | 24 | #1385 maps/mp/_utility, #1474 maps/mp/_movers, #1483, #1511, ... |
| 0x817a | method | (unnamed) | - | UNIMPLEMENTED | 24 | #1002, #1468, #1490, #1509, ... |
| 0x8308 | method | hasweapon | V | implemented | 24 | #1002, #1385 maps/mp/_utility, #1483, #1485, ... |
| 0x00c5 | function | (unnamed) | - | UNIMPLEMENTED | 23 | #1063, #1064, #1511, #1521, ... |
| 0x0136 | function | visionsetnaked | V | implemented | 23 | #1063, #1246, #1373 maps/mp/_load, #1476, ... |
| 0x013e | function | precachestring | V | implemented | 23 | #1002, #1063, #1493, #1494, ... |
| 0x024b | function | (unnamed) | - | UNIMPLEMENTED | 23 | #1503, #1553, #1554, maps/mp/gametypes/blitz, ... |
| 0x80b7 | method | setcontents | L | approximate | 23 | #1063, #1477, #1511, #1521, ... |
| 0x826e | method | useanimtree | L | implemented | 23 | #1351, #1373 maps/mp/_load, #1490, #1509, ... |
| 0x82c8 | method | unk_82c8 | G | no-op | 23 | #1063, #1385 maps/mp/_utility, #1511, #1512, ... |
| 0x8326 | method | (unnamed) | - | UNIMPLEMENTED | 23 | #1002, #1004, #1468, #1470, ... |
| 0x00e4 | function | length | V | implemented | 22 | #1068, #1379, #1385 maps/mp/_utility, #1519, ... |
| 0x0131 | function | isusingmatchrulesdata | L | approximate | 22 | #1385 maps/mp/_utility, maps/mp/gametypes/blitz, maps/mp/gametypes/conf, maps/mp/gametypes/cranked, ... |
| 0x017d | function | (unnamed) | - | UNIMPLEMENTED | 22 | #1477, #1519, #1524, #1551, ... |
| 0x0187 | function | unk_0187 | G | no-op | 22 | #1476, #981, #998, maps/mp/gametypes/horde |
| 0x8008 | method | setplayerangles | G | approximate | 22 | #1385 maps/mp/_utility, #1470, #1483, #1511, ... |
| 0x804f | method | (unnamed) | - | UNIMPLEMENTED | 22 | #1002, #1036, #1063, #1064, ... |
| 0x8092 | method | getorigin | V | implemented | 22 | #1002, #1006, #1036, #1061, ... |
| 0x80fc | method | (unnamed) | - | UNIMPLEMENTED | 22 | #1005, #1006, #1036, #1120, ... |
| 0x80fe | method | (unnamed) | - | UNIMPLEMENTED | 22 | #1006, #1036, #1521, #1540, ... |
| 0x8109 | method | (unnamed) | - | UNIMPLEMENTED | 22 | #1067, animscripts/animmode, animscripts/atv, animscripts/combat, ... |
| 0x8153 | method | (unnamed) | - | UNIMPLEMENTED | 22 | #1006, #1067, #1068, #1521, ... |
| 0x8289 | method | (unnamed) | - | UNIMPLEMENTED | 22 | #1068, #1120, #1519, #1523, ... |
| 0x82b4 | method | notsolid | V | implemented | 22 | #1063, #1067, #1068, #1373 maps/mp/_load, ... |
| 0x83fe | method | unk_83fe | G | no-op | 22 | #1476, #1488, #1493, #1507, ... |
| 0x006a | function | (unnamed) | - | UNIMPLEMENTED | 21 | #1006, #1036, #1067, #1068, ... |
| 0x01d0 | function | weaponclass | L | approximate | 21 | #1023, #1063, #1385 maps/mp/_utility, #1466, ... |
| 0x01e8 | function | isbot | L | implemented | 21 | #1371, #1385 maps/mp/_utility, #1468, #1478, ... |
| 0x80d1 | method | setcursorhint | V | no-op | 21 | #1002, #1036, #1379, #1470, ... |
| 0x8140 | method | (unnamed) | - | UNIMPLEMENTED | 21 | animscripts/atv, animscripts/combat, animscripts/cover_prone, animscripts/cover_wall, ... |
| 0x825b | method | (unnamed) | - | UNIMPLEMENTED | 21 | #1068, #1120, #1523, #1535, ... |
| 0x82c1 | method | unk_82c1 | G | no-op | 21 | #1385 maps/mp/_utility, #1488, #1489, #1495, ... |
| 0x0082 | function | sighttracepassed | L | approximate | 20 | #1006, #1036, #1466, #1500, ... |
| 0x015e | function | precachemodel | V | implemented | 20 | #1351, #1373 maps/mp/_load, #1519, maps/mp/gametypes/conf, ... |
| 0x80b8 | method | makeusable | L | implemented | 20 | #1002, #1006, #1379, #1385 maps/mp/_utility, ... |
| 0x80cf | method | unk_80cf | G | no-op | 20 | #1002, #1063, #1364, #1470, ... |
| 0x81a1 | method | (unnamed) | - | UNIMPLEMENTED | 20 | #1006, #1014, #1036, #1063, ... |
| 0x836a | method | (unnamed) | - | UNIMPLEMENTED | 20 | maps/mp/gametypes/horde |
| 0x018d | function | (unnamed) | - | UNIMPLEMENTED | 19 | #1375, #1512, #1519, #1523, ... |
| 0x80a6 | method | unk_80a6 | G | no-op | 19 | #1002, #1063, #1363, #1366, ... |
| 0x821f | method | getcommonplayerdata | V | approximate | 19 | #1355, #1465, #1476, #1494, ... |
| 0x832e | method | (unnamed) | - | UNIMPLEMENTED | 19 | #1063, #1359, #1512, #1516, ... |
| 0x833d | method | setscriptmoverkillcam | L | implemented | 19 | #1373 maps/mp/_load, #1511, #1519, #1520, ... |
| 0x8373 | method | (unnamed) | - | UNIMPLEMENTED | 19 | #1509, #1511, #1523, #1524, ... |
| 0x0134 | function | unk_0134 | G | no-op | 18 | #1505, maps/mp/gametypes/blitz, maps/mp/gametypes/conf, maps/mp/gametypes/cranked, ... |
| 0x01ac | function | tablelookupgetrownum | V | implemented | 18 | #1385 maps/mp/_utility, #1488, #1491, #1492, ... |
| 0x810b | method | (unnamed) | - | UNIMPLEMENTED | 18 | #1002, #1036, #1063, #1066, ... |
| 0x8136 | method | (unnamed) | - | UNIMPLEMENTED | 18 | #1006, #1011, #1036, #1067, ... |
| 0x8209 | method | (unnamed) | - | UNIMPLEMENTED | 18 | #1002, #962, #979, #981, ... |
| 0x8250 | method | (unnamed) | - | UNIMPLEMENTED | 18 | #1068, #1120, #1523, #1539, ... |
| 0x0018 | function | (unnamed) | - | UNIMPLEMENTED | 17 | #1063, #1068, #1512, #1521, ... |
| 0x0135 | function | unk_0135 | G | no-op | 17 | #1476, #1486, maps/mp/gametypes/sd, maps/mp/gametypes/siege, ... |
| 0x013f | function | precachemenu | V | implemented | 17 | #1002, #1493, #916 common_scripts/_createfx, #921, ... |
| 0x01ce | function | (unnamed) | - | UNIMPLEMENTED | 17 | #1477, #1519, #1524, #1535, ... |
| 0x8103 | method | (unnamed) | - | UNIMPLEMENTED | 17 | #1002, #947, animscripts/combat_utility, animscripts/corner, ... |
| 0x81a0 | method | (unnamed) | - | UNIMPLEMENTED | 17 | #1006, #1036, #1063, #960 |
| 0x82b2 | method | rotateto | V | implemented | 17 | #1024, #1474 maps/mp/_movers, #1511, #920, ... |
| 0x8302 | method | giveweapon | V | implemented | 17 | #1002, #1004, #1063, #1385 maps/mp/_utility, ... |
| 0x8306 | method | unk_8306 | G | no-op | 17 | #1004, #1470, #1483, #1484, ... |
| 0x0062 | function | unk_0062 | G | no-op | 16 | #1470, #1568, animscripts/cover_behavior, animscripts/death, ... |
| 0x0149 | function | spawnfx | V | approximate | 16 | #1483, #1537, #1550, #1561, ... |
| 0x0152 | function | (unnamed) | - | UNIMPLEMENTED | 16 | maps/mp/gametypes/blitz, maps/mp/gametypes/conf, maps/mp/gametypes/cranked, maps/mp/gametypes/dm, ... |
| 0x8045 | method | hidepart | L | implemented | 16 | #1036, #1068, #1511, #1519, ... |
| 0x80b3 | method | (unnamed) | - | UNIMPLEMENTED | 16 | #1512, #1514, #1516, #1532, ... |
| 0x80ff | method | (unnamed) | - | UNIMPLEMENTED | 16 | #1006, #1521, #1523, #1540, ... |
| 0x8172 | method | (unnamed) | - | UNIMPLEMENTED | 16 | #1036, #1063, #1511, #1521, ... |
| 0x818b | method | (unnamed) | - | UNIMPLEMENTED | 16 | animscripts/cover_arrival, animscripts/death, animscripts/move, animscripts/pain, ... |
| 0x8258 | method | (unnamed) | - | UNIMPLEMENTED | 16 | #1120, #1535, #1538, #1539, ... |
| 0x82cb | method | (unnamed) | - | UNIMPLEMENTED | 16 | #1385 maps/mp/_utility, #1482, #1516, #1535, ... |
| 0x0175 | function | (unnamed) | - | no-op | 15 | #1470, #1491, #1494, #1501, ... |
| 0x018f | function | isvehicle | G | approximate | 15 | #1036, #1063, #1068, #919 common_scripts/_destructible |
| 0x801f | method | unk_801f | G | no-op | 15 | #1036, #1067, #1470, #1511, ... |
| 0x8057 | method | disconnectpaths | V | implemented | 15 | #1002, #1061, #1063, #1068, ... |
| 0x80f9 | method | (unnamed) | - | UNIMPLEMENTED | 15 | #1512, #1521, #1523, #1540, ... |
| 0x813c | method | (unnamed) | - | UNIMPLEMENTED | 15 | #1002, #947, animscripts/atv, animscripts/combat, ... |
| 0x827b | method | (unnamed) | - | UNIMPLEMENTED | 15 | #1063, #1067, #1068, #1565, ... |
| 0x002e | function | (unnamed) | - | UNIMPLEMENTED | 14 | #1372, #1516, #1518, #1536 |
| 0x00cf | function | (unnamed) | - | UNIMPLEMENTED | 14 | #1002, #1006, #1011, #1036, ... |
| 0x0109 | function | (unnamed) | - | UNIMPLEMENTED | 14 | #1068, #1504, #1519, animscripts/combat, ... |
| 0x0186 | function | iprintlnbold | L | implemented | 14 | #1036, #1063, #1359, #1480, ... |
| 0x019d | function | (unnamed) | - | UNIMPLEMENTED | 14 | #1120, #1521, #1523, #1539, ... |
| 0x8007 | method | setorigin | G | implemented | 14 | #1385 maps/mp/_utility, #1470, #1483, #1519, ... |
| 0x8068 | method | enablelinkto | L | implemented | 14 | #1068, #1379, #1474 maps/mp/_movers, #1477, ... |
| 0x8079 | method | (unnamed) | - | UNIMPLEMENTED | 14 | #1020, #1063, #1064, #1375, ... |
| 0x80ce | method | setvalue | L | implemented | 14 | #1488, #916 common_scripts/_createfx, #967, #998, ... |
| 0x80d3 | method | (unnamed) | - | UNIMPLEMENTED | 14 | #1521, #1545, #1556, #1562, ... |
| 0x8148 | method | (unnamed) | - | UNIMPLEMENTED | 14 | #1002, #1068, #948, animscripts/atv, ... |
| 0x814b | method | (unnamed) | - | UNIMPLEMENTED | 14 | #1002, animscripts/corner, animscripts/move, animscripts/pain, ... |
| 0x81ab | method | (unnamed) | - | UNIMPLEMENTED | 14 | animscripts/combat, animscripts/death, animscripts/grenade_cower, animscripts/move, ... |
| 0x81c6 | method | (unnamed) | - | UNIMPLEMENTED | 14 | animscripts/combat, animscripts/cover_prone, animscripts/exit_node, animscripts/init, ... |
| 0x81d0 | method | radiusdamage | V | implemented | 14 | #1068, #1509, #1511, #1537, ... |
| 0x825e | method | (unnamed) | - | UNIMPLEMENTED | 14 | #1120, #1535, #1538, #1539, ... |
| 0x8291 | method | unk_8291 | G | no-op | 14 | #1476, #1480, #1493, #1500, ... |
| 0x82ff | method | (unnamed) | - | UNIMPLEMENTED | 14 | #1002, #1063, #1385 maps/mp/_utility, #1468, ... |
| 0x002f | function | isstring | V | implemented | 13 | #1068, #1476, #1510, #921, ... |
| 0x005a | function | getweaponmodel | G | approximate | 13 | #1483, #1492, #1527, #1571, ... |
| 0x0123 | function | getmatchdata | V | approximate | 13 | #1374, #1476, #1500 |
| 0x0150 | function | getteamscore | L | implemented | 13 | #1381, #1476, #1493, maps/mp/gametypes/blitz |
| 0x0210 | function | (unnamed) | - | UNIMPLEMENTED | 13 | #1483, #1490, #1532, #1539, ... |
| 0x8003 | method | playsoundtoteam | L | implemented | 13 | #1385 maps/mp/_utility, #1466, #1500, #1509, ... |
| 0x8056 | method | connectpaths | V | implemented | 13 | #1002, #1063, #1068, #1373 maps/mp/_load, ... |
| 0x8082 | method | scaleovertime | L | implemented | 13 | #1488, #1489, #981, #999 |
| 0x8083 | method | moveovertime | L | implemented | 13 | #1000, #1489, #981, #989, ... |
| 0x80b2 | method | (unnamed) | - | UNIMPLEMENTED | 13 | #1512, #1514, #1516, #1532, ... |
| 0x80d4 | method | (unnamed) | - | UNIMPLEMENTED | 13 | #1521, #1543, #1545, #1556, ... |
| 0x8131 | method | (unnamed) | - | UNIMPLEMENTED | 13 | #1512, #1521, #1523, #1540, ... |
| 0x819e | method | (unnamed) | - | UNIMPLEMENTED | 13 | #1036, #1067, #947, #955, ... |
| 0x81d9 | method | getlightintensity | L | implemented | 13 | #1001, #920 |
| 0x820a | method | (unnamed) | - | UNIMPLEMENTED | 13 | #1002, #979, #981, #998 |
| 0x825c | method | (unnamed) | - | UNIMPLEMENTED | 13 | #1068, #1538, #1539, #1541, ... |
| 0x8263 | method | iprintln | G | implemented | 13 | #1385 maps/mp/_utility, #1501 |
| 0x830a | method | (unnamed) | - | UNIMPLEMENTED | 13 | #1004, #1483, #1491, #1543, ... |
| 0x005b | function | getweaponattachments | L | approximate | 12 | #1385 maps/mp/_utility, #1492, #1494, #1498, ... |
| 0x00d6 | function | floor | V | implemented | 12 | #1063, #1385 maps/mp/_utility, #1476, #1500, ... |
| 0x018b | function | (unnamed) | - | UNIMPLEMENTED | 12 | #1375, #1515, #1516, #1519, ... |
| 0x0198 | function | newteamhudelem | L | implemented | 12 | #1364, #1470, #1472, #1484, ... |
| 0x01aa | function | tablelookupistring | L | approximate | 12 | #1385 maps/mp/_utility, #1501, #1508, #1548, ... |
| 0x01b4 | function | (unnamed) | - | UNIMPLEMENTED | 12 | #1063, #1375, #1503, #1554, ... |
| 0x8112 | method | (unnamed) | - | UNIMPLEMENTED | 12 | #1002, #1061, #1063, #963 |
| 0x81bf | method | (unnamed) | - | UNIMPLEMENTED | 12 | animscripts/corner, animscripts/cover_arrival, animscripts/exit_node, animscripts/melee, ... |
| 0x82cd | method | setblurforplayer | V | no-op | 12 | #1359, #1385 maps/mp/_utility, #1516, #1520, ... |
| 0x82dc | method | unk_82dc | G | no-op | 12 | #1385 maps/mp/_utility, #1495 |
| 0x830c | method | setoffhandsecondaryclass | L | implemented | 12 | #1004, #1385 maps/mp/_utility, #1509, #1559, ... |
| 0x8325 | method | givemaxammo | L | implemented | 12 | #1385 maps/mp/_utility, #1491, #1526, #1527, ... |
| 0x8334 | method | playerads | L | approximate | 12 | #1002, #1063, #1371, #1383, ... |
| 0x8004 | method | (unnamed) | - | UNIMPLEMENTED | 11 | #1485, #1524, #1554, #1569, ... |
| 0x80b1 | method | unk_80b1 | G | no-op | 11 | #1470, #1516, #1535, #1560, ... |
| 0x81b3 | method | (unnamed) | - | UNIMPLEMENTED | 11 | animscripts/combat_utility, animscripts/notetracks, animscripts/pain, animscripts/run, ... |
| 0x81d2 | method | (unnamed) | - | UNIMPLEMENTED | 11 | #1063, #1503, #1511, #968 |
| 0x81de | method | (unnamed) | - | UNIMPLEMENTED | 11 | #1511, #1514, #1516, #1532, ... |
| 0x825d | method | (unnamed) | - | UNIMPLEMENTED | 11 | #1120, #1538, #1539 |
| 0x82a7 | method | movegravity | L | approximate | 11 | #1006, #1024, #1516, #1520, ... |
| 0x832c | method | unk_832c | G | no-op | 11 | #1063, #1359, #1379, #963, ... |
| 0x83c6 | method | (unnamed) | - | UNIMPLEMENTED | 11 | #1063, #1483, #1484, #1519, ... |
| 0x8404 | method | playsoundonmovingent | L | implemented | 11 | #1474 maps/mp/_movers, #1512, #1520, #1538, ... |
| 0x0085 | function | playerphysicstrace | G | approximate | 10 | #1477, #1500, #1505, #1571, ... |
| 0x0089 | function | getnorthyaw | L | approximate | 10 | #1357 maps/mp/_compass, #961, #967, animscripts/battlechatter |
| 0x00b8 | function | getnodearray | V | approximate | 10 | #1002, #1014, #1036, #1063, ... |
| 0x00d7 | function | ceil | V | implemented | 10 | #1063, #1385 maps/mp/_utility, #1500, animscripts/move, ... |
| 0x013c | function | precacheitem | V | implemented | 10 | #1002, #1036, #1120, #1373 maps/mp/_load, ... |
| 0x0170 | function | physicsexplosionsphere | L | implemented | 10 | #1061, #1068, #1375, #1470, ... |
| 0x0185 | function | (unnamed) | - | UNIMPLEMENTED | 10 | #1063, #1067, #1068, #1476, ... |
| 0x8090 | method | (unnamed) | - | UNIMPLEMENTED | 10 | #1006, #1036, #1063, #1068, ... |
| 0x8091 | method | (unnamed) | - | UNIMPLEMENTED | 10 | #1036, #1063, #1068, #960, ... |
| 0x8111 | method | (unnamed) | - | UNIMPLEMENTED | 10 | #1002, #1061, #1063, #963 |
| 0x812e | method | (unnamed) | - | UNIMPLEMENTED | 10 | #1068, #1521, #1523, #1540, ... |
| 0x8137 | method | (unnamed) | - | UNIMPLEMENTED | 10 | #1006, #1067, #1068, #947 |
| 0x81c1 | method | (unnamed) | - | UNIMPLEMENTED | 10 | #1063, #1066, #1067, #947, ... |
| 0x8254 | method | (unnamed) | - | UNIMPLEMENTED | 10 | #1068, #1519, #1538, #1539 |
| 0x82ac | method | rotateyaw | V | implemented | 10 | #1061, #1516, #1545, #1567, ... |
| 0x82c3 | method | getweaponslistoffhands | L | implemented | 10 | #1521, #1538, #1545, #1548, ... |
| 0x82f3 | method | setclientdvars | L | implemented | 10 | #1476, #1500, #1501 |
| 0x831c | method | unk_831c | G | implemented | 10 | #1385 maps/mp/_utility, #1484, #999 |
| 0x83fd | method | setcamerapoint | G | implemented | 10 | #1500 |
| 0x00ce | function | (unnamed) | - | UNIMPLEMENTED | 9 | #1014, #1036, #1063, #954 |
| 0x0111 | function | (unnamed) | - | UNIMPLEMENTED | 9 | #1548, #1569, animscripts/death, animscripts/swim |
| 0x017b | function | (unnamed) | - | UNIMPLEMENTED | 9 | #1477, #1519, #1524, #1564, ... |
| 0x0190 | function | (unnamed) | - | UNIMPLEMENTED | 9 | #1120, #1516, #1523, #1535, ... |
| 0x8032 | method | (unnamed) | - | UNIMPLEMENTED | 9 | animscripts/corner, animscripts/cover_multi, animscripts/utility |
| 0x8086 | method | setpulsefx | L | implemented | 9 | #1488, #1489, #998, #999 |
| 0x80c4 | method | (unnamed) | - | UNIMPLEMENTED | 9 | #1470, #1477, #1484, #1562, ... |
| 0x810d | method | (unnamed) | - | UNIMPLEMENTED | 9 | #1002, #1067, #1068, #947, ... |
| 0x8178 | method | setstance | L | approximate | 9 | #1379, #1465, #1484, #1500, ... |
| 0x81c2 | method | (unnamed) | - | UNIMPLEMENTED | 9 | animscripts/combat, animscripts/cover_behavior, animscripts/move, animscripts/shared |
| 0x81d1 | method | (unnamed) | - | UNIMPLEMENTED | 9 | #1511, #1519, #1552, #968 |
| 0x81f5 | method | (unnamed) | - | UNIMPLEMENTED | 9 | animscripts/cover_prone, animscripts/notetracks, animscripts/setposemovement, animscripts/stop, ... |
| 0x823a | method | setplayerdata | V | approximate | 9 | #1023, #1564 |
| 0x8249 | method | (unnamed) | - | UNIMPLEMENTED | 9 | #1068, #1120, #1523, #1535, ... |
| 0x82ca | method | (unnamed) | - | UNIMPLEMENTED | 9 | #1385 maps/mp/_utility, #1514, #1532, #1548, ... |
| 0x82d4 | method | unk_82d4 | G | no-op | 9 | #1470, #1548, maps/mp/gametypes/infect, maps/mp/gametypes/sd, ... |
| 0x82db | method | (unnamed) | - | UNIMPLEMENTED | 9 | #1371, #1550 |
| 0x003d | function | (unnamed) | - | UNIMPLEMENTED | 8 | #1036, #1063, #1067, animscripts/notetracks, ... |
| 0x006b | function | (unnamed) | - | UNIMPLEMENTED | 8 | #1067, #947, animscripts/animmode |
| 0x0088 | function | (unnamed) | - | UNIMPLEMENTED | 8 | animscripts/corner, animscripts/init_move_transitions, animscripts/swim |
| 0x0133 | function | (unnamed) | - | no-op | 8 | #1002, #1063, #1385 maps/mp/_utility, #1480, ... |
| 0x0140 | function | (unnamed) | - | UNIMPLEMENTED | 8 | #1002, #1066 |
| 0x0158 | function | positionwouldtelefrag | L | implemented | 8 | #1379, #1500, #1504, #1505, ... |
| 0x8030 | method | (unnamed) | - | UNIMPLEMENTED | 8 | animscripts/corner, animscripts/cover_multi, animscripts/cover_wall |
| 0x807a | method | (unnamed) | - | UNIMPLEMENTED | 8 | #1063, #1064, #1387, #967 |
| 0x80d9 | method | (unnamed) | - | UNIMPLEMENTED | 8 | #1005, #1006, #1120 |
| 0x80e0 | method | (unnamed) | - | UNIMPLEMENTED | 8 | #1006, #1521, #1523, #1540, ... |
| 0x80fa | method | (unnamed) | - | UNIMPLEMENTED | 8 | #1521, #1545, #1556, #1562, ... |
| 0x8175 | method | (unnamed) | - | UNIMPLEMENTED | 8 | #1512, #1521, #1545, #1556, ... |
| 0x8181 | method | setdepthoffield | V | implemented | 8 | #1385 maps/mp/_utility, #1476, #1500, #949 |
| 0x81a2 | method | (unnamed) | - | UNIMPLEMENTED | 8 | #1036, #1061, #1063 |
| 0x81b4 | method | (unnamed) | - | UNIMPLEMENTED | 8 | animscripts/combat_utility, animscripts/notetracks, animscripts/pain, animscripts/run, ... |
| 0x81b8 | method | (unnamed) | - | UNIMPLEMENTED | 8 | #954, animscripts/combat, animscripts/door, animscripts/notetracks, ... |
| 0x81d7 | method | (unnamed) | - | UNIMPLEMENTED | 8 | #1515, #1518, #1521, #1538, ... |
| 0x81fc | method | (unnamed) | - | UNIMPLEMENTED | 8 | #1514, #1516, #1532, #1560 |
| 0x820f | method | (unnamed) | - | UNIMPLEMENTED | 8 | #1371, #1372, #1383, #1532 |
| 0x82b3 | method | solid | V | implemented | 8 | #1063, #1379, #1477, #919 common_scripts/_destructible, ... |
| 0x82d9 | method | (unnamed) | - | UNIMPLEMENTED | 8 | #1371, #1383, #1550, #1564 |
| 0x8304 | method | takeallweapons | L | implemented | 8 | #1004, #1063, #1359, #1468, ... |
| 0x8311 | method | (unnamed) | - | no-op | 8 | #1002, #1063, #1476, #1516, ... |
| 0x8331 | method | (unnamed) | - | UNIMPLEMENTED | 8 | #1477, #1512, #1516, #1550, ... |
| 0x8332 | method | (unnamed) | - | UNIMPLEMENTED | 8 | #1120, #1512, #1542, #1550, ... |
| 0x8337 | method | unk_8337 | G | no-op | 8 | #1004, #1508, #1547, maps/mp/gametypes/horde, ... |
| 0x8423 | method | unk_8423 | G | no-op | 8 | #1488, maps/mp/gametypes/blitz, maps/mp/gametypes/dom, maps/mp/gametypes/siege |
| 0x0020 | function | (unnamed) | - | UNIMPLEMENTED | 7 | #1002, #1068, #1557, #955, ... |
| 0x00f2 | function | (unnamed) | - | UNIMPLEMENTED | 7 | #1498 |
| 0x00fa | function | (unnamed) | - | UNIMPLEMENTED | 7 | #1036, #1063, #1511 |
| 0x013b | function | (unnamed) | - | UNIMPLEMENTED | 7 | #1002, #1063, #1066, #1359 |
| 0x0160 | function | exitlevel | L | implemented | 7 | #1467, #1476, #1501 |
| 0x018a | function | getspawnarray | V | approximate | 7 | #1375, #1504, #1505 |
| 0x01cb | function | resetscriptslot | G | no-op | 7 | #1385 maps/mp/_utility, #1477, maps/mp/gametypes/conf, maps/mp/gametypes/mugger, ... |
| 0x01d8 | function | getvehiclenodearray | V | approximate | 7 | #1063, #1565, #927 common_scripts/utility |
| 0x0207 | function | (unnamed) | - | UNIMPLEMENTED | 7 | maps/mp/gametypes/mugger |
| 0x8009 | method | (unnamed) | - | UNIMPLEMENTED | 7 | #1533, #1534, #1564, #1571 |
| 0x802a | method | (unnamed) | - | UNIMPLEMENTED | 7 | #1067, #1068, #1511, #911 |
| 0x802b | method | (unnamed) | - | UNIMPLEMENTED | 7 | #1067, #1068, #1511, #911 |
| 0x8041 | method | (unnamed) | - | UNIMPLEMENTED | 7 | #1001 |
| 0x8069 | method | (unnamed) | - | UNIMPLEMENTED | 7 | animscripts/face, animscripts/notetracks |
| 0x80ac | method | unk_80ac | G | no-op | 7 | #1373 maps/mp/_load, #1385 maps/mp/_utility, #1521, #1523, ... |
| 0x80af | method | (unnamed) | - | UNIMPLEMENTED | 7 | #1516, #1535, #1560, #1562, ... |
| 0x80d8 | method | (unnamed) | - | UNIMPLEMENTED | 7 | #1005, #1006, #1036, #1120 |
| 0x8104 | method | (unnamed) | - | UNIMPLEMENTED | 7 | animscripts/animmode, animscripts/atv, animscripts/combat, animscripts/combat_utility, ... |
| 0x8133 | method | (unnamed) | - | UNIMPLEMENTED | 7 | #1002, #1509, #1521, #1545, ... |
| 0x8179 | method | (unnamed) | - | UNIMPLEMENTED | 7 | #1002, #1511 |
| 0x8185 | method | (unnamed) | - | UNIMPLEMENTED | 7 | #1006, #1036, #1067, animscripts/cover_wall |
| 0x818d | method | (unnamed) | - | UNIMPLEMENTED | 7 | animscripts/atv, animscripts/combat, animscripts/cover_arrival, animscripts/shoot_behavior, ... |
| 0x8199 | method | (unnamed) | - | UNIMPLEMENTED | 7 | animscripts/traverse/iw6_jumpup_72, animscripts/traverse/ladder_down, animscripts/traverse/ladder_up, animscripts/traverse/shared |
| 0x81c0 | method | (unnamed) | - | UNIMPLEMENTED | 7 | #1067, #947, animscripts/animmode, animscripts/traverse/iw6_jumpup_72, ... |
| 0x81d3 | method | (unnamed) | - | UNIMPLEMENTED | 7 | #1120, #1538, #1539, #1565 |
| 0x81dd | method | (unnamed) | - | UNIMPLEMENTED | 7 | #1511, #1514, #1516, #1532, ... |
| 0x8224 | method | (unnamed) | - | UNIMPLEMENTED | 7 | #1068 |
| 0x8246 | method | (unnamed) | - | UNIMPLEMENTED | 7 | #1511, #1527, maps/mp/gametypes/sotf, maps/mp/gametypes/sotf_ffa |
| 0x8252 | method | (unnamed) | - | UNIMPLEMENTED | 7 | #1068, #1539 |
| 0x826f | method | scriptmodelclearanim | L | implemented | 7 | #1373 maps/mp/_load, #1509, #1568 |
| 0x832d | method | (unnamed) | - | UNIMPLEMENTED | 7 | #1063, #1470, #1494 |
| 0x8336 | method | (unnamed) | - | UNIMPLEMENTED | 7 | #1385 maps/mp/_utility, #1521, #1563, #1564, ... |
| 0x8338 | method | setoffhandprimaryclass | L | implemented | 7 | #1385 maps/mp/_utility, #981 |
| 0x83a7 | method | unk_83a7 | G | no-op | 7 | #1508, #1547, maps/mp/gametypes/horde, maps/mp/gametypes/infect |
| 0x000e | function | (unnamed) | - | UNIMPLEMENTED | 6 | #1002, #1064 |
| 0x0084 | function | physicstrace | L | approximate | 6 | #1067, #1068, #1516, #927 common_scripts/utility, ... |
| 0x00a5 | function | setslowmotion | L | implemented | 6 | #1063, #1492, #1553 |
| 0x00af | function | (unnamed) | - | UNIMPLEMENTED | 6 | #967, animscripts/battlechatter, animscripts/cover_arrival |
| 0x00bb | function | unk_00bb | G | no-op | 6 | #1063, #1483, #1506, #1554 |
| 0x0112 | function | (unnamed) | - | UNIMPLEMENTED | 6 | #1372, animscripts/combat_utility, animscripts/swim, animscripts/utility |
| 0x0174 | function | (unnamed) | - | UNIMPLEMENTED | 6 | #1063, #1553, #949, #967 |
| 0x01d2 | function | (unnamed) | - | UNIMPLEMENTED | 6 | #1002, #1511, #1525 |
| 0x01d3 | function | (unnamed) | - | UNIMPLEMENTED | 6 | #1002, #1490, #1494, #1511 |
| 0x8002 | method | (unnamed) | - | UNIMPLEMENTED | 6 | #1538, #1539 |
| 0x800b | method | (unnamed) | - | UNIMPLEMENTED | 6 | #1533, #1534, #1571 |
| 0x800e | method | (unnamed) | - | UNIMPLEMENTED | 6 | #1477 |
| 0x8015 | method | unk_8015 | G | no-op | 6 | #1470, #1506, #1509, #1524, ... |
| 0x802c | method | (unnamed) | - | UNIMPLEMENTED | 6 | #1067, #1068, #1511, #911 |
| 0x8031 | method | (unnamed) | - | UNIMPLEMENTED | 6 | #947, animscripts/cover_arrival, animscripts/cover_behavior, animscripts/utility |
| 0x807b | method | (unnamed) | - | UNIMPLEMENTED | 6 | #1516, #1535, #1561, #1563, ... |
| 0x8088 | method | (unnamed) | - | UNIMPLEMENTED | 6 | #1487, #1489, #981, #999 |
| 0x80ab | method | unk_80ab | G | no-op | 6 | #1373 maps/mp/_load, #1385 maps/mp/_utility, #1521, #1523, ... |
| 0x80fb | method | (unnamed) | - | UNIMPLEMENTED | 6 | #1521, #1523, #1563 |
| 0x8155 | method | (unnamed) | - | UNIMPLEMENTED | 6 | #1512, #1521, #1562 |
| 0x8159 | method | (unnamed) | - | UNIMPLEMENTED | 6 | #1006, animscripts/cover_arrival, animscripts/swim |
| 0x8195 | method | (unnamed) | - | UNIMPLEMENTED | 6 | #947, animscripts/melee, animscripts/move, animscripts/reactions |
| 0x81aa | method | unk_81aa | G | no-op | 6 | #1380, #919 common_scripts/_destructible |
| 0x81ba | method | (unnamed) | - | UNIMPLEMENTED | 6 | animscripts/combat, animscripts/cover_arrival, animscripts/door, animscripts/utility |
| 0x8201 | method | (unnamed) | - | UNIMPLEMENTED | 6 | #1512, #1542, #1554, #1562, ... |
| 0x8202 | method | (unnamed) | - | UNIMPLEMENTED | 6 | #1512, #1542, #1554, #1562, ... |
| 0x82da | method | (unnamed) | - | UNIMPLEMENTED | 6 | #1371, #1383, #1550, #1564 |
| 0x82fc | method | setweaponhudiconoverrides | V | implemented | 6 | #1002, #1004, #1385 maps/mp/_utility |
| 0x8310 | method | (unnamed) | - | no-op | 6 | #1002, #1063, #927 common_scripts/utility |
| 0x839e | method | (unnamed) | - | UNIMPLEMENTED | 6 | #1531, maps/mp/agents/_scriptedagents |
| 0x83b1 | method | (unnamed) | - | UNIMPLEMENTED | 6 | animscripts/run, maps/mp/agents/_scriptedagents |
| 0x8406 | method | unk_8406 | G | no-op | 6 | #1385 maps/mp/_utility, #1466, #1482 |
| 0x004f | function | (unnamed) | - | UNIMPLEMENTED | 5 | #979 |
| 0x0071 | function | (unnamed) | - | UNIMPLEMENTED | 5 | #1063, #1064 |
| 0x00b9 | function | (unnamed) | - | UNIMPLEMENTED | 5 | #1002, #1036, #1505, #967 |
| 0x00da | function | sqrt | V | implemented | 5 | #1014, #1520, #927 common_scripts/utility, animscripts/combat, ... |
| 0x014f | function | (unnamed) | - | UNIMPLEMENTED | 5 | maps/mp/gametypes/cranked, maps/mp/gametypes/dm, maps/mp/gametypes/gun, maps/mp/gametypes/mugger, ... |
| 0x0159 | function | unk_0159 | G | no-op | 5 | #1379, #1484, #1505, #1506, ... |
| 0x016b | function | (unnamed) | - | UNIMPLEMENTED | 5 | #1567, #1568, maps/mp/gametypes/infect |
| 0x01fe | function | (unnamed) | - | UNIMPLEMENTED | 5 | maps/mp/gametypes/blitz, maps/mp/gametypes/dom, maps/mp/gametypes/siege |
| 0x023b | function | unk_023b | G | no-op | 5 | #1476, #1486 |
| 0x800a | method | unk_800a | G | implemented | 5 | #1385 maps/mp/_utility, #1523, #1547, #1571 |
| 0x8067 | method | (unnamed) | - | UNIMPLEMENTED | 5 | #1563, #981, maps/mp/gametypes/blitz, maps/mp/gametypes/grnd |
| 0x806e | method | (unnamed) | - | UNIMPLEMENTED | 5 | #1063, #952 |
| 0x80a5 | method | (unnamed) | - | UNIMPLEMENTED | 5 | #1063, #1509, #952, animscripts/battlechatter |
| 0x80d7 | method | (unnamed) | - | no-op | 5 | #1351, #923 common_scripts/_fx, #927 common_scripts/utility |
| 0x80e1 | method | (unnamed) | - | UNIMPLEMENTED | 5 | #1006, #1036 |
| 0x8107 | method | (unnamed) | - | UNIMPLEMENTED | 5 | #947, animscripts/move |
| 0x812f | method | (unnamed) | - | UNIMPLEMENTED | 5 | #1521, #1523, #1562, #1563, ... |
| 0x8135 | method | (unnamed) | - | UNIMPLEMENTED | 5 | #947, animscripts/animmode, animscripts/melee |
| 0x813a | method | (unnamed) | - | UNIMPLEMENTED | 5 | #1014, #1063, #1067, #947 |
| 0x816e | method | (unnamed) | - | UNIMPLEMENTED | 5 | animscripts/combat, animscripts/cover_prone, animscripts/init, animscripts/run |
| 0x8173 | method | (unnamed) | - | UNIMPLEMENTED | 5 | #1063, #967, animscripts/combat_utility |
| 0x8186 | method | (unnamed) | - | UNIMPLEMENTED | 5 | #1006, #1036 |
| 0x81a4 | method | (unnamed) | - | UNIMPLEMENTED | 5 | #1036, #1067, #960 |
| 0x81a5 | method | (unnamed) | - | UNIMPLEMENTED | 5 | #1036 |
| 0x81fd | method | (unnamed) | - | UNIMPLEMENTED | 5 | #1512, #1542, #1562, #1564, ... |
| 0x8261 | method | suicide | L | implemented | 5 | #1385 maps/mp/_utility, #1493 |
| 0x8265 | method | unk_8265 | G | no-op | 5 | #1358, #1500 |
| 0x826d | method | clonebrushmodeltoscriptmodel | V | no-op | 5 | #1519, #1557, maps/mp/mp_frag |
| 0x8274 | method | (unnamed) | - | UNIMPLEMENTED | 5 | #1068, #1565 |
| 0x82b7 | method | physicslaunchserver | L | no-op | 5 | #919 common_scripts/_destructible, #920, #922 common_scripts/_exploder, animscripts/death, ... |
| 0x82c6 | method | (unnamed) | - | UNIMPLEMENTED | 5 | #1509, #1521, #1545, #1556, ... |
| 0x8316 | method | openpopupmenu | L | implemented | 5 | #1002, #1493, #1571, #916 common_scripts/_createfx, ... |
| 0x8333 | method | (unnamed) | - | UNIMPLEMENTED | 5 | #1477, #1511, #1524 |
| 0x841f | method | setrankedplayerdatareservedint | V | approximate | 5 | #1498, #1501 |
| 0x001f | function | (unnamed) | - | UNIMPLEMENTED | 4 | #1002, #1557, #955, animscripts/combat_utility |
| 0x007a | function | (unnamed) | - | UNIMPLEMENTED | 4 | #1063, #954 |
| 0x0091 | function | (unnamed) | - | UNIMPLEMENTED | 4 | #1120 |
| 0x00e0 | function | pointonsegmentnearesttopoint | L | implemented | 4 | #1520, #920, animscripts/pain |
| 0x00e5 | function | (unnamed) | - | UNIMPLEMENTED | 4 | #1504, maps/mp/agents/_scriptedagents, maps/mp/gametypes/blitz |
| 0x00f8 | function | (unnamed) | - | UNIMPLEMENTED | 4 | #1002, #980 |
| 0x011b | function | (unnamed) | - | UNIMPLEMENTED | 4 | #1567 |
| 0x011f | function | (unnamed) | - | UNIMPLEMENTED | 4 | #1567, #1568 |
| 0x017e | function | (unnamed) | - | UNIMPLEMENTED | 4 | #1524, #1535, maps/mp/gametypes/conf, maps/mp/gametypes/grnd |
| 0x019f | function | (unnamed) | - | UNIMPLEMENTED | 4 | animscripts/combat_utility, animscripts/cover_prone, animscripts/death, animscripts/weaponlist |
| 0x01a3 | function | (unnamed) | - | UNIMPLEMENTED | 4 | animscripts/combat_utility, animscripts/shared, animscripts/shoot_behavior, animscripts/weaponlist |
| 0x01ab | function | (unnamed) | - | UNIMPLEMENTED | 4 | maps/mp/gametypes/sotf, maps/mp/gametypes/sotf_ffa |
| 0x01b1 | function | (unnamed) | - | UNIMPLEMENTED | 4 | #1371, #1520, #1538 |
| 0x020f | function | (unnamed) | - | UNIMPLEMENTED | 4 | #1002 |
| 0x023a | function | unk_023a | G | no-op | 4 | #1476, #1478, maps/mp/gametypes/horde |
| 0x0255 | function | (unnamed) | - | UNIMPLEMENTED | 4 | #1553 |
| 0x8025 | method | (unnamed) | - | UNIMPLEMENTED | 4 | #1036, #1068, #1516, #975 |
| 0x8048 | method | showpart | L | implemented | 4 | #919 common_scripts/_destructible, animscripts/notetracks, animscripts/shared |
| 0x804e | method | viewkick | G | implemented | 4 | #1063, #1379, #1511, #1519 |
| 0x80a8 | method | unk_80a8 | G | no-op | 4 | #1064, #1068, #1500 |
| 0x80b5 | method | (unnamed) | - | UNIMPLEMENTED | 4 | #1571 |
| 0x80b6 | method | (unnamed) | - | UNIMPLEMENTED | 4 | #1568 |
| 0x80c6 | method | settimer | L | implemented | 4 | #1385 maps/mp/_utility, #1484, #1571, maps/mp/gametypes/grnd |
| 0x80da | method | (unnamed) | - | UNIMPLEMENTED | 4 | #1006, #1521, #1523, #1541 |
| 0x80df | method | (unnamed) | - | UNIMPLEMENTED | 4 | #1512, #1562, #1563 |
| 0x8123 | method | (unnamed) | - | UNIMPLEMENTED | 4 | #946, #951, #953 |
| 0x812b | method | (unnamed) | - | UNIMPLEMENTED | 4 | #1063, #1068 |
| 0x8158 | method | (unnamed) | - | UNIMPLEMENTED | 4 | #1006, animscripts/combat, animscripts/combat_utility |
| 0x8174 | method | (unnamed) | - | UNIMPLEMENTED | 4 | #1521, #1562 |
| 0x819b | method | (unnamed) | - | UNIMPLEMENTED | 4 | animscripts/door |
| 0x81c5 | method | (unnamed) | - | UNIMPLEMENTED | 4 | #1014, #1067 |
| 0x81ca | method | (unnamed) | - | UNIMPLEMENTED | 4 | animscripts/combat_utility, animscripts/door |
| 0x81d6 | method | (unnamed) | - | UNIMPLEMENTED | 4 | #1120, #1372 |
| 0x81db | method | unk_81db | G | no-op | 4 | #1067, #1470, animscripts/shared |
| 0x81dc | method | setmovespeedscale | L | implemented | 4 | #1064, #1358, #1511, maps/mp/gametypes/infect |
| 0x81df | method | (unnamed) | - | UNIMPLEMENTED | 4 | animscripts/cover_arrival, animscripts/swim |
| 0x81e5 | method | (unnamed) | - | UNIMPLEMENTED | 4 | animscripts/combat_utility, animscripts/run, animscripts/snowmobile |
| 0x8218 | method | (unnamed) | - | UNIMPLEMENTED | 4 | #1533, #1534 |
| 0x8234 | method | (unnamed) | - | UNIMPLEMENTED | 4 | #998 |
| 0x8245 | method | setspawnweapon | L | implemented | 4 | #1385 maps/mp/_utility, #1468, maps/mp/gametypes/gun, maps/mp/gametypes/infect |
| 0x8270 | method | scriptmodelplayanim | L | implemented | 4 | #1520, #1558, maps/mp/mp_frag |
| 0x8280 | method | (unnamed) | - | UNIMPLEMENTED | 4 | #1068 |
| 0x8286 | method | (unnamed) | - | UNIMPLEMENTED | 4 | #1068 |
| 0x8294 | method | (unnamed) | - | UNIMPLEMENTED | 4 | #1478, #1500, maps/mp/gametypes/mugger |
| 0x8296 | method | unk_8296 | G | no-op | 4 | #1501 |
| 0x82ab | method | rotatepitch | V | implemented | 4 | #1551, #955 |
| 0x82ad | method | rotateroll | V | implemented | 4 | #1382, maps/mp/mp_frag |
| 0x82ae | method | devaddyaw | L | no-op | 4 | #1064, #1551, #916 common_scripts/_createfx |
| 0x82b6 | method | setcanradiusdamage | L | implemented | 4 | maps/mp/mp_frag |
| 0x82e2 | method | (unnamed) | - | UNIMPLEMENTED | 4 | #1477, #1562, #1563 |
| 0x82e3 | method | (unnamed) | - | UNIMPLEMENTED | 4 | #1477, #1562, #1563 |
| 0x82e7 | method | (unnamed) | - | UNIMPLEMENTED | 4 | #1064, #1359, #1543 |
| 0x8315 | method | (unnamed) | - | no-op | 4 | #1516, #1543, #1548, #927 common_scripts/utility |
| 0x8375 | method | (unnamed) | - | no-op | 4 | #1474 maps/mp/_movers, maps/mp/mp_frag |
| 0x8405 | method | unk_8405 | G | no-op | 4 | #1063, #1379, #1543, maps/mp/gametypes/blitz |
| 0x841e | method | getrankedplayerdatareservedint | V | approximate | 4 | #1498, #1501 |
| 0x8422 | method | (unnamed) | - | UNIMPLEMENTED | 4 | #1493, #1500, #1507 |
| 0x0031 | function | (unnamed) | - | UNIMPLEMENTED | 3 | #1385 maps/mp/_utility, #1466, #1553 |
| 0x0050 | function | (unnamed) | - | UNIMPLEMENTED | 3 | #979 |
| 0x007d | function | (unnamed) | - | UNIMPLEMENTED | 3 | #954 |
| 0x00d1 | function | (unnamed) | - | UNIMPLEMENTED | 3 | #1002, #960 |
| 0x00dd | function | (unnamed) | - | UNIMPLEMENTED | 3 | #1569, animscripts/stop, animscripts/utility |
| 0x00f1 | function | bullettracer | G | no-op | 3 | #1505, #1506 |
| 0x00f4 | function | (unnamed) | - | UNIMPLEMENTED | 3 | #1002, #1064 |
| 0x00f7 | function | (unnamed) | - | UNIMPLEMENTED | 3 | #1064, #954 |
| 0x00fc | function | (unnamed) | - | UNIMPLEMENTED | 3 | #967, animscripts/combat_utility |
| 0x00fd | function | (unnamed) | - | UNIMPLEMENTED | 3 | #1063 |
| 0x00ff | function | (unnamed) | - | UNIMPLEMENTED | 3 | #1063, #1511 |
| 0x010a | function | (unnamed) | - | UNIMPLEMENTED | 3 | #1001 |
| 0x010f | function | (unnamed) | - | UNIMPLEMENTED | 3 | #949, animscripts/swim |
| 0x0139 | function | (unnamed) | - | UNIMPLEMENTED | 3 | #951 |
| 0x0141 | function | (unnamed) | - | UNIMPLEMENTED | 3 | #1512, #1520, #1537 |
| 0x0151 | function | setteamscore | L | implemented | 3 | #1478, maps/mp/gametypes/infect |
| 0x015f | function | map_restart | L | implemented | 3 | #1476 |
| 0x017c | function | (unnamed) | - | UNIMPLEMENTED | 3 | #1507, #1558 |
| 0x0194 | function | (unnamed) | - | UNIMPLEMENTED | 3 | #1120, animscripts/shared |
| 0x01a0 | function | (unnamed) | - | UNIMPLEMENTED | 3 | animscripts/shoot_behavior, animscripts/weaponlist |
| 0x01a1 | function | (unnamed) | - | UNIMPLEMENTED | 3 | #981, animscripts/utility |
| 0x01a4 | function | weaponinventorytype | G | approximate | 3 | #1063, #1511, animscripts/shared |
| 0x01db | function | (unnamed) | - | UNIMPLEMENTED | 3 | #1066, #1540 |
| 0x021e | function | unk_021e | G | no-op | 3 | #1002, #1470, #1506 |
| 0x0220 | function | soundgetplaybacktime | G | approximate | 3 | #1063, #1474 maps/mp/_movers |
| 0x024f | function | unk_024f | G | no-op | 3 | #1476 |
| 0x8022 | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1385 maps/mp/_utility, #1468 |
| 0x8029 | method | unk_8029 | G | no-op | 3 | #1468, #911 |
| 0x8072 | method | playsoundasmaster | V | approximate | 3 | #920, #927 common_scripts/utility |
| 0x807e | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1379, #1470, #1477 |
| 0x808f | method | unk_808f | G | no-op | 3 | #1063, #1379 |
| 0x80a3 | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1466, #1527, animscripts/battlechatter |
| 0x80de | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1512, #1562, #1563 |
| 0x80ef | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1063, maps/mp/gametypes/horde |
| 0x8110 | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1002, #1063 |
| 0x8122 | method | (unnamed) | - | UNIMPLEMENTED | 3 | #946, #951 |
| 0x8125 | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1020, #954 |
| 0x8142 | method | (unnamed) | - | UNIMPLEMENTED | 3 | animscripts/cover_prone, animscripts/cover_wall |
| 0x8146 | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1063, #1067, animscripts/corner |
| 0x814c | method | (unnamed) | - | UNIMPLEMENTED | 3 | #947, animscripts/run, animscripts/swim |
| 0x814e | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1521, #1523 |
| 0x814f | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1521, #1523 |
| 0x8154 | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1006 |
| 0x815c | method | (unnamed) | - | UNIMPLEMENTED | 3 | animscripts/animset, animscripts/combat, animscripts/reactions |
| 0x815f | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1036, #1063 |
| 0x817f | method | unk_817f | G | no-op | 3 | #1500, #1563 |
| 0x81ad | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1511 |
| 0x81b0 | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1120, #1523, #1569 |
| 0x81bb | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1516, animscripts/cover_arrival, animscripts/run |
| 0x81bc | method | (unnamed) | - | UNIMPLEMENTED | 3 | animscripts/combat, animscripts/shoot_behavior, animscripts/track |
| 0x81c8 | method | (unnamed) | - | UNIMPLEMENTED | 3 | animscripts/squadmanager, animscripts/utility |
| 0x81e2 | method | (unnamed) | - | UNIMPLEMENTED | 3 | animscripts/notetracks, animscripts/utility |
| 0x81eb | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1006, animscripts/combat_utility, animscripts/cover_behavior |
| 0x81ed | method | (unnamed) | - | UNIMPLEMENTED | 3 | animscripts/combat |
| 0x81f6 | method | (unnamed) | - | UNIMPLEMENTED | 3 | animscripts/combat_utility, animscripts/cover_prone, animscripts/pain |
| 0x81fa | method | (unnamed) | - | UNIMPLEMENTED | 3 | animscripts/pain, animscripts/reactions |
| 0x81fb | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1514, #1532, #1560 |
| 0x821d | method | getprivateplayerdata | V | approximate | 3 | #1385 maps/mp/_utility, #1501, #1508 |
| 0x826b | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1519 |
| 0x8277 | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1467, #1538, #1565 |
| 0x8292 | method | getspectatingplayer | L | approximate | 3 | #1385 maps/mp/_utility, #1507, maps/mp/gametypes/blitz |
| 0x829b | method | setperk | V | implemented | 3 | #1385 maps/mp/_utility, #1564 |
| 0x829e | method | unsetperk | V | implemented | 3 | #1385 maps/mp/_utility, #1547, #1564 |
| 0x82a8 | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1063, animscripts/atv, animscripts/snowmobile |
| 0x82af | method | devaddroll | L | no-op | 3 | #1064, #916 common_scripts/_createfx |
| 0x82b0 | method | devaddpitch | L | no-op | 3 | #1064, #916 common_scripts/_createfx |
| 0x82c2 | method | getweaponslistprimaries | L | implemented | 3 | #1004, #1509, #1511 |
| 0x82de | method | unk_82de | G | no-op | 3 | #1466, #1506 |
| 0x82e0 | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1505, #1571 |
| 0x82fd | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1571 |
| 0x8314 | method | (unnamed) | - | no-op | 3 | #1470, #1543, #927 common_scripts/utility |
| 0x8328 | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1002, #954 |
| 0x8376 | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1519, #1569 |
| 0x839f | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1531, maps/mp/agents/_scriptedagents |
| 0x83b9 | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1379, #1490 |
| 0x83ba | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1385 maps/mp/_utility |
| 0x83c4 | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1511 |
| 0x83e9 | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1571 |
| 0x83ec | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1497, #1511 |
| 0x83ed | method | (unnamed) | - | UNIMPLEMENTED | 3 | #1497, #1511 |
| 0x83fa | method | unk_83fa | G | no-op | 3 | #1374 |
| 0x841d | method | unk_841d | G | no-op | 3 | #1501 |
| 0x0001 | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1066, #1540 |
| 0x002a | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1002, #979 |
| 0x0041 | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1063 |
| 0x0070 | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1063 |
| 0x007b | function | (unnamed) | - | UNIMPLEMENTED | 2 | #954 |
| 0x007c | function | (unnamed) | - | UNIMPLEMENTED | 2 | #954 |
| 0x007e | function | (unnamed) | - | UNIMPLEMENTED | 2 | #954 |
| 0x007f | function | (unnamed) | - | UNIMPLEMENTED | 2 | #954 |
| 0x0086 | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1490, #1511 |
| 0x00ad | function | (unnamed) | - | UNIMPLEMENTED | 2 | animscripts/combat, animscripts/shared |
| 0x00cc | function | (unnamed) | - | UNIMPLEMENTED | 2 | animscripts/combat_utility |
| 0x00d2 | function | (unnamed) | - | UNIMPLEMENTED | 2 | animscripts/init, animscripts/shared |
| 0x00d4 | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1063, #981 |
| 0x00ec | function | visionsetthermal | L | implemented | 2 | #1002, #1373 maps/mp/_load |
| 0x00ed | function | visionsetpain | L | implemented | 2 | #1002, #1373 maps/mp/_load |
| 0x00f9 | function | (unnamed) | - | UNIMPLEMENTED | 2 | #949 |
| 0x0114 | function | isendstr | V | implemented | 2 | #1385 maps/mp/_utility, #1511 |
| 0x0118 | function | strcmp | G | implemented | 2 | #927 common_scripts/utility |
| 0x0119 | function | (unnamed) | - | UNIMPLEMENTED | 2 | #951 |
| 0x011a | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1567 |
| 0x011c | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1567 |
| 0x011d | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1567 |
| 0x012a | function | getclientmatchdata | L | approximate | 2 | #1370, #1381 |
| 0x012f | function | unk_012f | G | no-op | 2 | #1374, #1501 |
| 0x0132 | function | kick | L | no-op | 2 | #1358, #1500 |
| 0x0155 | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1500 |
| 0x015c | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1475 |
| 0x0169 | function | unk_0169 | G | no-op | 2 | #1476, #1500 |
| 0x016d | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1567, maps/mp/gametypes/infect |
| 0x016f | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1567 |
| 0x017f | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1519, #1558 |
| 0x0180 | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1519, #1558 |
| 0x0183 | function | (unnamed) | - | UNIMPLEMENTED | 2 | maps/mp/gametypes/sotf, maps/mp/gametypes/sotf_ffa |
| 0x019c | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1063, animscripts/death |
| 0x01a2 | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1572 |
| 0x01ad | function | (unnamed) | - | UNIMPLEMENTED | 2 | #953 |
| 0x01ae | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1466, #1509 |
| 0x01be | function | setminimap | L | implemented | 2 | #1357 maps/mp/_compass, #961 |
| 0x01c2 | function | getglass | G | approximate | 2 | #1002, #1367 |
| 0x01c6 | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1061 |
| 0x020e | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1063 |
| 0x022c | function | (unnamed) | - | UNIMPLEMENTED | 2 | animscripts/swim |
| 0x0257 | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1531 |
| 0x025d | function | (unnamed) | - | UNIMPLEMENTED | 2 | #1506, #1569 |
| 0x8005 | method | unk_8005 | G | no-op | 2 | #1470, #1511 |
| 0x8016 | method | unk_8016 | G | no-op | 2 | #1470 |
| 0x802f | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1063 |
| 0x8035 | method | (unnamed) | - | UNIMPLEMENTED | 2 | animscripts/combat_utility, animscripts/pain |
| 0x8038 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1063 |
| 0x804b | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1067, animscripts/melee |
| 0x804d | method | (unnamed) | - | UNIMPLEMENTED | 2 | #981 |
| 0x8054 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1002, #1036 |
| 0x8055 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1002, #1036 |
| 0x807d | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1063, #967 |
| 0x8093 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1006, #1067 |
| 0x80a7 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1064, #1068 |
| 0x80c5 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1477 |
| 0x811b | method | (unnamed) | - | UNIMPLEMENTED | 2 | #951 |
| 0x8132 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1002, #968 |
| 0x813f | method | (unnamed) | - | UNIMPLEMENTED | 2 | #947 |
| 0x8151 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1521, #1523 |
| 0x8157 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1063, #960 |
| 0x815a | method | (unnamed) | - | UNIMPLEMENTED | 2 | animscripts/combat, animscripts/utility |
| 0x8164 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1063 |
| 0x8167 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #960 |
| 0x8168 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #960 |
| 0x8176 | method | unk_8176 | G | no-op | 2 | #920, #998 |
| 0x8184 | method | (unnamed) | - | UNIMPLEMENTED | 2 | animscripts/grenade_return_throw |
| 0x818c | method | (unnamed) | - | UNIMPLEMENTED | 2 | animscripts/exit_node, animscripts/swim |
| 0x818f | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1067, animscripts/battlechatter |
| 0x819c | method | (unnamed) | - | UNIMPLEMENTED | 2 | animscripts/door |
| 0x819f | method | (unnamed) | - | UNIMPLEMENTED | 2 | animscripts/combat_utility |
| 0x81b7 | method | (unnamed) | - | UNIMPLEMENTED | 2 | animscripts/utility |
| 0x81cb | method | (unnamed) | - | UNIMPLEMENTED | 2 | animscripts/combat_utility |
| 0x81ce | method | (unnamed) | - | UNIMPLEMENTED | 2 | animscripts/combat_utility, animscripts/grenade_return_throw |
| 0x8216 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1571 |
| 0x8219 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1063 |
| 0x8223 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1068, #1565 |
| 0x8225 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1068 |
| 0x8229 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1066, #1068 |
| 0x8253 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1068, #1539 |
| 0x8267 | method | cloneplayer | L | implemented | 2 | #1470, #1511 |
| 0x8272 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1068, #1565 |
| 0x8275 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1066, #1068 |
| 0x8279 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1068 |
| 0x828b | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1538, #1539 |
| 0x828c | method | getxuid | L | approximate | 2 | #1476, #1500 |
| 0x8293 | method | unk_8293 | G | no-op | 2 | #1500 |
| 0x8295 | method | unk_8295 | G | no-op | 2 | #1478 |
| 0x82bc | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1484, #1500 |
| 0x82bd | method | unk_82bd | G | no-op | 2 | #1500 |
| 0x82bf | method | unk_82bf | G | implemented | 2 | #1385 maps/mp/_utility, #1547 |
| 0x82c7 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1562, #1564 |
| 0x82d0 | method | (unnamed) | - | UNIMPLEMENTED | 2 | animscripts/shared |
| 0x82d8 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1371 |
| 0x82dd | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1505 |
| 0x82f7 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1543 |
| 0x82fb | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1571 |
| 0x8307 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1004, #1483 |
| 0x8312 | method | (unnamed) | - | no-op | 2 | #1470, #927 common_scripts/utility |
| 0x831e | method | unk_831e | G | implemented | 2 | #927 common_scripts/utility |
| 0x8352 | method | unk_8352 | G | no-op | 2 | #1470 |
| 0x835f | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1516 |
| 0x836b | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1516 |
| 0x8372 | method | (unnamed) | - | UNIMPLEMENTED | 2 | maps/mp/gametypes/sr |
| 0x8386 | method | (unnamed) | - | UNIMPLEMENTED | 2 | maps/mp/agents/_scriptedagents |
| 0x83a6 | method | (unnamed) | - | UNIMPLEMENTED | 2 | animscripts/combat_utility |
| 0x83a8 | method | (unnamed) | - | UNIMPLEMENTED | 2 | animscripts/notetracks |
| 0x83a9 | method | (unnamed) | - | UNIMPLEMENTED | 2 | animscripts/notetracks |
| 0x83af | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1511 |
| 0x83b7 | method | unk_83b7 | G | no-op | 2 | #1364 |
| 0x83c5 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1523 |
| 0x83c7 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1484 |
| 0x83ce | method | (unnamed) | - | UNIMPLEMENTED | 2 | animscripts/death |
| 0x83e8 | method | unk_83e8 | G | no-op | 2 | #1492 |
| 0x83ea | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1571 |
| 0x83f1 | method | unk_83f1 | G | no-op | 2 | #1500 |
| 0x83f3 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #951 |
| 0x83fb | method | unk_83fb | G | no-op | 2 | #1374 |
| 0x8402 | method | getlinkedchildren | L | implemented | 2 | #1379, #1519 |
| 0x8408 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1503, #1554 |
| 0x840c | method | unk_840c | G | no-op | 2 | #1494, #1511 |
| 0x840e | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1542, #1554 |
| 0x8411 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1543 |
| 0x8416 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1532 |
| 0x8417 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1548 |
| 0x8418 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1494 |
| 0x841c | method | setcommonplayerdatareservedint | V | approximate | 2 | #1381 |
| 0x8424 | method | (unnamed) | - | UNIMPLEMENTED | 2 | #1531 |
| 0x0004 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #998 |
| 0x0019 | function | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/cover_wall |
| 0x001d | function | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/utility |
| 0x0021 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1068 |
| 0x003a | function | (unnamed) | - | UNIMPLEMENTED | 1 | #949 |
| 0x0051 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1063 |
| 0x005f | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1036 |
| 0x0065 | function | (unnamed) | - | no-op | 1 | #927 common_scripts/utility |
| 0x006d | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1002 |
| 0x006f | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1063 |
| 0x0076 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #951 |
| 0x008b | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1064 |
| 0x008e | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1006 |
| 0x009c | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1120 |
| 0x00ae | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1359 |
| 0x00ba | function | unk_00ba | G | no-op | 1 | #1379 |
| 0x00d0 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1067 |
| 0x00df | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1359 |
| 0x00e7 | function | length2dsquared | V | implemented | 1 | #927 common_scripts/utility |
| 0x00ee | function | unk_00ee | G | no-op | 1 | #1476 |
| 0x0100 | function | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/move |
| 0x0105 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1063 |
| 0x0110 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1379 |
| 0x011e | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1567 |
| 0x0124 | function | sendmatchdata | L | no-op | 1 | #1476 |
| 0x0126 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1374 |
| 0x0127 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1500 |
| 0x0128 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1374 |
| 0x012b | function | setclientmatchdatadef | L | no-op | 1 | #1476 |
| 0x012c | function | sendclientmatchdata | L | no-op | 1 | #1476 |
| 0x012d | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1374 |
| 0x012e | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1374 |
| 0x0137 | function | visionsetnight | V | implemented | 1 | #1373 maps/mp/_load |
| 0x0138 | function | visionsetmissilecam | V | implemented | 1 | #1373 maps/mp/_load |
| 0x0142 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1480 |
| 0x0147 | function | killfxontag | L | implemented | 1 | #947 |
| 0x0148 | function | playloopedfx | V | approximate | 1 | #923 common_scripts/_fx |
| 0x014c | function | unk_014c | G | no-op | 1 | #1476 |
| 0x0157 | function | unk_0157 | G | no-op | 1 | #1470 |
| 0x016a | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1476 |
| 0x016c | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1477 |
| 0x0173 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1063 |
| 0x0178 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1520 |
| 0x017a | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1063 |
| 0x0181 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1507 |
| 0x0182 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1507 |
| 0x0184 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1507 |
| 0x0192 | function | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/shared |
| 0x01b2 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1371 |
| 0x01b3 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1565 |
| 0x01c3 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1061 |
| 0x01c5 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1367 |
| 0x01d5 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1020 |
| 0x01d6 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1002 |
| 0x01d9 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1068 |
| 0x01dc | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1562 |
| 0x01dd | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1063 |
| 0x01e0 | function | botsconnecttype | G | approximate | 1 | #1385 maps/mp/_utility |
| 0x01ea | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1531 |
| 0x01fc | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1506 |
| 0x0200 | function | connectnodepair | V | no-op | 1 | maps/mp/mp_frag |
| 0x0201 | function | disconnectnodepair | V | no-op | 1 | maps/mp/mp_frag |
| 0x0209 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1063 |
| 0x020a | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1063 |
| 0x020b | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1063 |
| 0x0212 | function | activateclientexploder | L | implemented | 1 | #922 common_scripts/_exploder |
| 0x0222 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1063 |
| 0x022a | function | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/swim |
| 0x022b | function | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/swim |
| 0x022d | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1512 |
| 0x0233 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1512 |
| 0x0235 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1512 |
| 0x0243 | function | playsoundtoplayers | G | implemented | 1 | #1385 maps/mp/_utility |
| 0x0248 | function | unk_0248 | G | no-op | 1 | #1571 |
| 0x0250 | function | unk_0250 | G | no-op | 1 | #1476 |
| 0x0251 | function | unk_0251 | G | no-op | 1 | #1476 |
| 0x0252 | function | (unnamed) | - | UNIMPLEMENTED | 1 | #1476 |
| 0x800c | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1571 |
| 0x800f | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1477 |
| 0x8010 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1477 |
| 0x8011 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1477 |
| 0x8012 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1500 |
| 0x8013 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1500 |
| 0x8017 | method | unk_8017 | G | no-op | 1 | #1470 |
| 0x8019 | method | attach | L | implemented | 1 | #1385 maps/mp/_utility |
| 0x8020 | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/death |
| 0x8021 | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/death |
| 0x8024 | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/death |
| 0x8027 | method | detach | L | implemented | 1 | #1385 maps/mp/_utility |
| 0x8028 | method | moveshieldmodel | L | no-op | 1 | #1385 maps/mp/_utility |
| 0x8034 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #955 |
| 0x8039 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1067 |
| 0x803a | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1036 |
| 0x803c | method | (unnamed) | - | UNIMPLEMENTED | 1 | #975 |
| 0x803e | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1002 |
| 0x8040 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1001 |
| 0x805a | method | (unnamed) | - | UNIMPLEMENTED | 1 | #975 |
| 0x8065 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1521 |
| 0x8066 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1067 |
| 0x8077 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1063 |
| 0x807c | method | (unnamed) | - | UNIMPLEMENTED | 1 | #967 |
| 0x8080 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #962 |
| 0x8096 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1521 |
| 0x8099 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1563 |
| 0x809a | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1563 |
| 0x80b4 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1571 |
| 0x80bb | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1061 |
| 0x80c2 | method | unk_80c2 | G | no-op | 1 | #916 common_scripts/_createfx |
| 0x80cc | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1063 |
| 0x80d0 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #962 |
| 0x80e2 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1063 |
| 0x80e5 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #979 |
| 0x80e6 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1063 |
| 0x80f2 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1063 |
| 0x80f3 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1063 |
| 0x8108 | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/cover_prone |
| 0x810a | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/combat_utility |
| 0x8116 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #951 |
| 0x8118 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #951 |
| 0x811a | method | (unnamed) | - | UNIMPLEMENTED | 1 | #951 |
| 0x811c | method | (unnamed) | - | UNIMPLEMENTED | 1 | #951 |
| 0x811d | method | (unnamed) | - | UNIMPLEMENTED | 1 | #951 |
| 0x811e | method | (unnamed) | - | UNIMPLEMENTED | 1 | #951 |
| 0x8126 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #954 |
| 0x8127 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #954 |
| 0x8134 | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/door |
| 0x8150 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1521 |
| 0x815b | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/combat |
| 0x815d | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/cover_behavior |
| 0x817e | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1470 |
| 0x8187 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1006 |
| 0x8196 | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/melee |
| 0x8197 | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/utility |
| 0x81a3 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1036 |
| 0x81a7 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #960 |
| 0x81ae | method | unk_81ae | G | no-op | 1 | #920 |
| 0x81b5 | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/shared |
| 0x81b6 | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/track |
| 0x81bd | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/shared |
| 0x81c9 | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/combat |
| 0x81d8 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #927 common_scripts/utility |
| 0x81e1 | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/exit_node |
| 0x81e4 | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/melee |
| 0x81e6 | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/cover_behavior |
| 0x81e9 | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/combat_utility |
| 0x81ec | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/cover_arrival |
| 0x81ee | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/combat_utility |
| 0x81f0 | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/combat_utility |
| 0x81f2 | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/combat |
| 0x81f3 | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/utility |
| 0x81f4 | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/utility |
| 0x8203 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1562 |
| 0x8204 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1542 |
| 0x8205 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1542 |
| 0x8208 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1063 |
| 0x820c | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1535 |
| 0x8213 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1061 |
| 0x821b | method | getplayerdata | V | approximate | 1 | #1023 |
| 0x8220 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1535 |
| 0x8232 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #981 |
| 0x8233 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #981 |
| 0x8235 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #998 |
| 0x8238 | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/scripted |
| 0x823c | method | setprivateplayerdata | V | approximate | 1 | #1385 maps/mp/_utility |
| 0x8247 | method | unk_8247 | G | no-op | 1 | #1511 |
| 0x824a | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1068 |
| 0x824b | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1068 |
| 0x824c | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1068 |
| 0x824d | method | (unnamed) | - | UNIMPLEMENTED | 1 | #980 |
| 0x824e | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1068 |
| 0x824f | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1068 |
| 0x8255 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1539 |
| 0x8256 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1068 |
| 0x8257 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1068 |
| 0x8259 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1068 |
| 0x825a | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1565 |
| 0x825f | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1535 |
| 0x8260 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1470 |
| 0x826a | method | getguid | V | approximate | 1 | #1385 maps/mp/_utility |
| 0x8271 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #947 |
| 0x8276 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1067 |
| 0x827c | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1068 |
| 0x827e | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1068 |
| 0x8282 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1068 |
| 0x8288 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1068 |
| 0x828d | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1500 |
| 0x828e | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1500 |
| 0x828f | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1500 |
| 0x8290 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1500 |
| 0x8298 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1371 |
| 0x8299 | method | unk_8299 | G | no-op | 1 | #1492 |
| 0x829d | method | clearperks | V | implemented | 1 | #1385 maps/mp/_utility |
| 0x829f | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1500 |
| 0x82a0 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1465 |
| 0x82c0 | method | unk_82c0 | G | no-op | 1 | #1547 |
| 0x82c4 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1470 |
| 0x82c5 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1571 |
| 0x82d5 | method | unk_82d5 | G | no-op | 1 | #1500 |
| 0x82d6 | method | (unnamed) | - | no-op | 1 | #1385 maps/mp/_utility |
| 0x82d7 | method | (unnamed) | - | no-op | 1 | #1385 maps/mp/_utility |
| 0x82df | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1466 |
| 0x82e4 | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/combat_utility |
| 0x82e5 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1063 |
| 0x82e6 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1063 |
| 0x82f0 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1511 |
| 0x82f4 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1505 |
| 0x82f5 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1505 |
| 0x830b | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1004 |
| 0x830d | method | getoffhandsecondaryclass | L | implemented | 1 | #1385 maps/mp/_utility |
| 0x830e | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1385 maps/mp/_utility |
| 0x830f | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1385 maps/mp/_utility |
| 0x8313 | method | (unnamed) | - | no-op | 1 | #927 common_scripts/utility |
| 0x8318 | method | closepopupmenu | L | implemented | 1 | #916 common_scripts/_createfx |
| 0x831d | method | (unnamed) | - | no-op | 1 | #927 common_scripts/utility |
| 0x831f | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1354 |
| 0x8320 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1354 |
| 0x8321 | method | unk_8321 | G | no-op | 1 | #1354 |
| 0x8322 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #951 |
| 0x833e | method | unk_833e | G | no-op | 1 | #927 common_scripts/utility |
| 0x833f | method | unk_833f | G | no-op | 1 | #1465 |
| 0x8340 | method | unk_8340 | G | no-op | 1 | #1465 |
| 0x8341 | method | (unnamed) | - | UNIMPLEMENTED | 1 | maps/mp/gametypes/horde |
| 0x835d | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1516 |
| 0x836d | method | unk_836d | G | no-op | 1 | #1385 maps/mp/_utility |
| 0x836e | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1516 |
| 0x8379 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1511 |
| 0x837b | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1531 |
| 0x837e | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1531 |
| 0x8399 | method | (unnamed) | - | UNIMPLEMENTED | 1 | animscripts/notetracks |
| 0x83a2 | method | (unnamed) | - | UNIMPLEMENTED | 1 | maps/mp/agents/_scriptedagents |
| 0x83a4 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1494 |
| 0x83ae | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1511 |
| 0x83b2 | method | (unnamed) | - | UNIMPLEMENTED | 1 | maps/mp/agents/_scriptedagents |
| 0x83bd | method | (unnamed) | - | UNIMPLEMENTED | 1 | #951 |
| 0x83bf | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1523 |
| 0x83c0 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1064 |
| 0x83c1 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1064 |
| 0x83d8 | method | unk_83d8 | G | no-op | 1 | #1508 |
| 0x83d9 | method | unk_83d9 | G | no-op | 1 | #1508 |
| 0x83da | method | unk_83da | G | no-op | 1 | #1508 |
| 0x83dc | method | (unnamed) | - | UNIMPLEMENTED | 1 | #949 |
| 0x83dd | method | (unnamed) | - | UNIMPLEMENTED | 1 | #949 |
| 0x83de | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1519 |
| 0x83e4 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1532 |
| 0x83e5 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1532 |
| 0x83eb | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1512 |
| 0x83ee | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1497 |
| 0x83fc | method | unk_83fc | G | no-op | 1 | #1385 maps/mp/_utility |
| 0x840d | method | unk_840d | G | no-op | 1 | #1511 |
| 0x840f | method | (unnamed) | - | UNIMPLEMENTED | 1 | maps/mp/gametypes/horde |
| 0x8410 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1543 |
| 0x8414 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1545 |
| 0x8415 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1545 |
| 0x8419 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1524 |
| 0x841b | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1553 |
| 0x8427 | method | (unnamed) | - | UNIMPLEMENTED | 1 | #1500 |

reachable functions 1198 of 9934; effect paths named: 87
  fx fx/explosions/grenadeexp_default
  fx fx/explosions/transformer_explosion
  fx fx/explosions/transformer_spark_runner
  fx fx/explosions/tv_explosion
  fx fx/fire/firelp_barrel_pm
  fx fx/fire/firelp_med_pm
  fx fx/fire/firelp_small_pm
  fx fx/fire/jet_afterburner
  fx fx/fire/tire_fire_med
  fx fx/fire/transformer_small_blacksmoke_fire
  fx fx/lights/lights_red_sm
  fx fx/lights/vfx_light_util_square
  fx fx/misc/aircraft_light_red_blink
  fx fx/misc/aircraft_light_wingtip_green
  fx fx/misc/aircraft_light_wingtip_red
  fx fx/misc/flare_ambient
  fx fx/misc/fluorescent_glow
  fx fx/misc/glow_stick_glow_pile
  fx fx/misc/glow_stick_glow_pile_orange
  fx fx/misc/insects_carcass_flies_heavy
  fx fx/misc/light_glow_industrial
  fx fx/misc/light_glow_orange_generic_pulse
  fx fx/misc/light_glow_red_generic_pulse
  fx fx/misc/light_glow_walllight_white
  fx fx/misc/light_glow_white
  fx fx/misc/light_glow_white_bulb
  fx fx/misc/light_glow_white_lamp
  fx fx/misc/lighthaze
  fx fx/misc/lighthaze_bog_a
  fx fx/misc/lighthaze_oilrig
  fx fx/misc/lighthaze_villassault
  fx fx/misc/lighthaze_white
  fx fx/misc/outdoor_motion_light
  fx fx/misc/power_tower_light_red_blink
  fx fx/misc/power_tower_light_red_blink_large
  fx fx/misc/tower_light_blue_steady
  fx fx/misc/tower_light_orange_steady
  fx fx/misc/tower_light_red_steady
  fx fx/props/glow_latern
  fx fx/props/icbm_mainframe_lightblink
  fx fx/props/metal_detector_light
  fx fx/smoke/amb_int_haze_frag
  fx fx/smoke/car_damage_blacksmoke
  fx fx/smoke/car_damage_whitesmoke
  fx fx/snow/snow_light_mp
  fx vfx/ambient/atmospheric/vfx_dock_mist
  fx vfx/ambient/atmospheric/vfx_dock_mist_close
  fx vfx/ambient/atmospheric/vfx_fog_water_mp
  fx vfx/ambient/atmospheric/vfx_frag_mist_distant
  fx vfx/ambient/atmospheric/vfx_int_haze_sz
  fx vfx/ambient/atmospheric/vfx_sunflare_frag
  fx vfx/ambient/dust/vfx_dirt_trail_falling_thick_frag
  fx vfx/ambient/dust/vfx_dirt_trail_falling_thin
  fx vfx/ambient/dust/vfx_dust_indoor_frag
  fx vfx/ambient/fire/fuel/vfx_fire_furnace
  fx vfx/ambient/fire/vfx_fire_barrel_small_nolight
  fx vfx/ambient/lights/smoky_fluorescent_light_fit_frag_ug
  fx vfx/ambient/lights/vfx_bulb_prismatic_mp_snow
  fx vfx/ambient/lights/vfx_glow_ceil_light_sov
  fx vfx/ambient/lights/vfx_glow_red_light_blink_frag
  fx vfx/ambient/lights/vfx_glow_red_light_frag
  fx vfx/ambient/lights/vfx_godray_frag_anim
  fx vfx/ambient/lights/vfx_godray_frag_anim_solid
  fx vfx/ambient/lights/vfx_godray_frag_anim_solid_long
  fx vfx/ambient/lights/vfx_godray_frag_anim_solid_ori
  fx vfx/ambient/lights/vfx_godray_frag_anim_streak
  fx vfx/ambient/lights/vfx_godray_frag_anim_streak2
  fx vfx/ambient/lights/vfx_godray_frag_anim_window
  fx vfx/ambient/lights/vfx_godray_frag_window_streak
  fx vfx/ambient/lights/vfx_light_blink_distance_red
  fx vfx/ambient/misc/vfx_spiral_trash
  fx vfx/ambient/misc/vfx_trash_frag
  fx vfx/ambient/props/vfx_handflare_sov
  fx vfx/ambient/smoke/vfx_smk_puffy_white_01
  fx vfx/ambient/smoke/vfx_smk_stack_vista
  fx vfx/ambient/smoke/vfx_smk_stack_white
  fx vfx/ambient/sparks/vfx_sparks_sign
  fx vfx/ambient/steam/vfx_steam_escape
  fx vfx/ambient/steam/vfx_steam_wispy_frag
  fx vfx/ambient/steam/vfx_steam_wispy_nitro
  fx vfx/ambient/water/drip_5x5_mp
  fx vfx/ambient/water/vfx_sprinkler_system_frag
  fx vfx/apex/nv_dog_a
  fx vfx/apex/nv_wolf_b
  fx vfx/apex/nv_wolf_c
  fx vfx/gameplay/explosions/weap/gre/vfx_exp_gre_dirt_cg
  fx vfx/moments/mp_strikezone/vfx_mparticlulate_sz

boot: BootReport { scripts: 313, script_errors: [], entities: 288, structs: 307, unresolved_includes: 16, main_found: true }
script errors in 10 s: 0
builtins hit and turned away: {}
