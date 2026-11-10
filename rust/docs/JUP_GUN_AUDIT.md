# Guest gun first-person fit audit

Output of `cargo run -p assets --example jup_gun_audit -- <zone dir> [gun filter]` (crates/assets/src/lane/jup_gun_audit.rs): each guest gun is assembled as `jup_gun` does and measured against its Black Ops II donor (axis, bore, magazine roll, grip and forend against the donor's palms, length). With a filter, each matching gun's parts are listed too (boxes, reach, frames, whether placed by root bone and how far that is from the listed tag).

Part zones were decompressed from the MW3 install. Run the example on Windows with the install's `zone` folder to cover them.

## What the audit found

- Attach frames and root-bone placement are sound: all 237 parts of the 61 audited guns sit on frames turned at most 0.3 degrees, and every part placed by its root bone lands within 0.5 units of its listed tag except the Whiskey's revolver cylinder (6.8, where the root is the right one: the cylinder lines up with the barrel's chamber). No part is detached from its receiver.
- Magazine roll near 180 or +-33/160 degrees is not a mis-turned magazine: those are top- and side-fed magazines (P90, Five-seven, Kriss, the HSierra's side box) and a revolver cylinder. Pistols' 10-25 degree axis pitch and the barrels' bore tilt come from principal-axis fits over grips, sights and rails, not from a tilted mesh.
- The real error was the forend: the donor's left palm sat a mean 7.5 units behind the gun's forend (worst +20.8 on long barrels), the gun reading longer than its donor. `view_fit` now draws a gun smaller about its grip (at most to 0.8x) until its forend is in the donor's left hand.

## Before and after

| measure | before (mean / worst) | after (mean / worst) |
|---|---|---|
| forend vs left palm, ahead | 7.46 / +20.77 | 4.79 / +15.34 |
| grip vs right palm, ahead | 1.67 / +6.42 | 1.49 / +5.49 |
| grip vs right palm, up | 1.14 / +4.82 | 1.69 / +5.66 |
| forend vs left palm, up | 4.22 / +7.07 | 4.56 / +7.29 |
| everything else | unchanged | unchanged |

(The grip rises a little because a smaller gun hangs lower about the sights; the sights still meet the donor's exactly.)

## Report after the fix

guns audited: 61 of 138
gun axis yaw (deg): n 61 mean |x| 0.28 worst -1.46
gun axis pitch (deg): n 61 mean |x| 4.89 worst +24.51
bore yaw (deg): n 61 mean |x| 0.52 worst +5.85
bore pitch (deg): n 61 mean |x| 2.20 worst +10.88
magazine roll (deg): n 59 mean |x| 19.18 worst -179.47
grip vs right palm, ahead: n 40 mean |x| 1.49 worst +5.49
grip vs right palm, across: n 40 mean |x| 1.25 worst +1.78
grip vs right palm, up: n 40 mean |x| 1.69 worst +5.66
forend vs left palm, ahead: n 61 mean |x| 4.79 worst +15.34
forend vs left palm, up: n 61 mean |x| 4.56 worst +7.29
receiver top vs donor sights: n 61 mean |x| 0.00 worst +0.00
turned attach frames (deg): n 237 mean |x| 0.00 worst +0.30
guns with a flag: 59

### Modern Warfare III (2023)

| gun | donor | yaw / pitch | bore yaw / pitch | roll | grip vs right palm (ahead, across, up) | forend vs left palm (ahead, up) | top vs sights | length vs donor | flags |
|---|---|---|---|---|---|---|---|---|---|
| `jp12_pi_mike93` | beretta93r | 0.4 / 19.7 | 0.1 / -2.7 | -6.9 | +1.9, +1.1, +0.3 | +7.6, +5.4 | +0.0 | 0.95x | gun axis off forward (yaw 0.4, pitch 19.7); forend +7.6 ahead, +5.4 up of the donor's left palm |
| `jp36_ar_anov94` | an94 | -0.1 / 1.4 | -0.0 / 3.2 | 0.2 | - | +3.9, +3.7 | +0.0 | 0.66x | forend +3.9 ahead, +3.7 up of the donor's left palm |
| `jp19_ar_acharlie` | hk416 | -0.0 / 1.7 | -0.0 / 2.6 | 0.4 | +3.9, +0.9, +2.0 | +2.6, +3.4 | +0.0 | 0.82x | grip +3.9 ahead, +2.0 up of the donor's right palm; forend +2.6 ahead, +3.4 up of the donor's left palm |
| `jp34_ar_balpha27` | scar | 0.1 / 0.7 | -0.0 / -3.0 | -175.5 | - | -2.6, +3.5 | +0.0 | 0.80x | magazine hangs -175 deg off down; forend -2.6 ahead, +3.5 up of the donor's left palm |
| `jp05_ar_foxtrot2000` | xm8 | 0.1 / 7.5 | -0.6 / -0.2 | -0.6 | +1.5, +1.3, -0.2 | -0.3, +3.8 | +0.0 | 0.83x | gun axis off forward (yaw 0.1, pitch 7.5); forend -0.3 ahead, +3.8 up of the donor's left palm |
| `jp08_ar_qbravo97` | type95 | -0.1 / 7.0 | 0.7 / -1.5 | 0.3 | - | +3.3, +5.9 | +0.0 | 0.66x | gun axis off forward (yaw -0.1, pitch 7.0); forend +3.3 ahead, +5.9 up of the donor's left palm |
| `jp44_ar_stango44` | tar21 | -0.1 / 3.2 | 0.0 / 1.4 | 1.6 | +1.8, +1.1, +1.7 | +7.5, +5.4 | +0.0 | 0.93x | forend +7.5 ahead, +5.4 up of the donor's left palm |
| `jp19_br_acharlie450` | scar | 0.2 / 1.1 | -0.5 / 1.0 | 0.4 | +5.5, +1.4, +3.2 | +2.6, +4.2 | +0.0 | 0.85x | grip +5.5 ahead, +3.2 up of the donor's right palm; forend +2.6 ahead, +4.2 up of the donor's left palm |
| `jp03_br_aromeo200` | scar | 0.2 / 2.7 | 0.1 / -4.4 | 0.0 | - | +2.9, +4.5 | +0.0 | 0.83x | forend +2.9 ahead, +4.5 up of the donor's left palm |
| `jp37_dm_kappa98` | ballista | -0.1 / 3.2 | -0.3 / 1.6 | - | - | +11.6, +4.7 | +0.0 | 0.76x | stock is 1.7x the receiver's size; forend +11.6 ahead, +4.7 up of the donor's left palm |
| `jp06_lm_pkilop` | mk48 | 0.1 / 0.3 | 0.0 / 0.8 | -33.4 | - | +3.7, +5.0 | +0.0 | 0.91x | magazine hangs -33 deg off down; forend +3.7 ahead, +5.0 up of the donor's left palm |
| `jp08_lm_qbravo95lsw` | qbb95 | 0.1 / 7.3 | 0.1 / 0.7 | 0.0 | - | +0.9, +4.7 | +0.0 | 0.48x | gun axis off forward (yaw 0.1, pitch 7.3); forend +0.9 ahead, +4.7 up of the donor's left palm; length 23 against the donor's 48 |
| `jp38_sh_spapa12` | saiga12 | -0.0 / 2.3 | 0.0 / -2.4 | 1.7 | -0.4, +1.4, +3.2 | +2.4, +4.8 | +0.0 | 0.75x | grip -0.4 ahead, +3.2 up of the donor's right palm; forend +2.4 ahead, +4.8 up of the donor's left palm |
| `jp41_sm_kpappa50` | pdw57 | -1.2 / 2.3 | -0.0 / -0.1 | 159.7 | -0.3, +1.8, +0.8 | -0.0, +3.6 | +0.0 | 1.23x | magazine hangs 160 deg off down; forend -0.0 ahead, +3.6 up of the donor's left palm |
| `jp25_sm_talpham` | mp7 | -0.3 / 11.6 | 0.7 / 2.5 | 0.2 | - | +1.5, +2.3 | +0.0 | 0.70x | gun axis off forward (yaw -0.3, pitch 11.6) |
| `jp10_sn_cdelta50` | svu | -0.1 / 5.9 | -0.0 / -0.0 | 0.8 | +0.3, +1.4, +1.3 | +15.3, +5.4 | +0.0 | 1.13x | barrel reaches 1.6 receiver lengths from its tag; forend +15.3 ahead, +5.4 up of the donor's left palm |
| `jp17_sn_hsierra` | dsr50 | -1.5 / -2.0 | 0.0 / -1.9 | 118.6 | +0.4, +0.7, +1.9 | +11.5, +7.2 | +0.0 | 0.92x | magazine hangs 119 deg off down; forend +11.5 ahead, +7.2 up of the donor's left palm |

### Modern Warfare II (2022)

| gun | donor | yaw / pitch | bore yaw / pitch | roll | grip vs right palm (ahead, across, up) | forend vs left palm (ahead, up) | top vs sights | length vs donor | flags |
|---|---|---|---|---|---|---|---|---|---|
| `iw9_ar_mike4` | hk416 | 0.1 / 4.6 | -0.2 / 3.4 | 1.2 | -0.2, +1.1, +0.5 | -0.2, +3.7 | +0.0 | 0.70x | barrel reaches 1.6 receiver lengths from its tag; forend -0.2 ahead, +3.7 up of the donor's left palm |
| `iw9_ar_mike16` | hk416 | 0.0 / 2.2 | 0.0 / 1.1 | 0.9 | -0.2, +1.1, +0.6 | +2.4, +3.5 | +0.0 | 0.90x | barrel reaches 2.5 receiver lengths from its tag; barrel is 2.1x the receiver's size; forend +2.4 ahead, +3.5 up of the donor's left palm |
| `iw9_ar_akilo` | an94 | -0.1 / 2.8 | 0.1 / 1.1 | 0.0 | +0.2, +1.8, +2.7 | +3.6, +5.2 | +0.0 | 0.68x | forend +3.6 ahead, +5.2 up of the donor's left palm |
| `iw9_ar_akilo105` | an94 | -0.1 / 3.8 | 0.1 / 1.8 | 0.1 | +0.2, +1.8, +2.7 | +2.2, +5.2 | +0.0 | 0.63x | forend +2.2 ahead, +5.2 up of the donor's left palm |
| `iw9_ar_akilo74` | an94 | -0.5 / -8.6 | -0.1 / -0.1 | -1.1 | +0.2, +1.5, +2.5 | +0.0, +5.0 | +0.0 | 0.52x | gun axis off forward (yaw -0.5, pitch -8.6); forend +0.0 ahead, +5.0 up of the donor's left palm; length 23 against the donor's 44 |
| `iw9_ar_kilo53` | hk416 | 0.1 / -0.9 | 0.4 / 4.4 | -0.1 | +3.4, +0.9, +2.9 | +2.4, +4.1 | +0.0 | 0.71x | grip +3.4 ahead, +2.9 up of the donor's right palm; forend +2.4 ahead, +4.1 up of the donor's left palm |
| `iw9_ar_golf3` | scar | -0.3 / 1.1 | -0.1 / 0.4 | -1.0 | +4.8, +1.3, +4.2 | +5.6, +5.1 | +0.0 | 0.92x | grip +4.8 ahead, +4.2 up of the donor's right palm; forend +5.6 ahead, +5.1 up of the donor's left palm |
| `iw9_ar_scharlie` | scar | 0.3 / -0.8 | 0.0 / 0.0 | -0.9 | - | +4.2, +3.3 | +0.0 | 0.80x | forend +4.2 ahead, +3.3 up of the donor's left palm |
| `iw9_ar_mcbravo` | hk416 | -0.6 / 1.2 | -2.9 / 10.9 | 0.5 | -0.5, +1.0, +0.7 | +0.1, +3.6 | +0.0 | 0.66x | bore off forward (yaw -2.9, pitch 10.9); forend +0.1 ahead, +3.6 up of the donor's left palm |
| `iw9_ar_mcharlie` | hk416 | -0.4 / -0.3 | -0.9 / 9.9 | 0.1 | -0.3, +1.0, +0.5 | -0.1, +3.5 | +0.0 | 0.74x | bore off forward (yaw -0.9, pitch 9.9); forend -0.1 ahead, +3.5 up of the donor's left palm |
| `iw9_ar_falpha` | xm8 | 0.1 / 5.5 | -1.6 / 0.1 | -0.4 | +0.0, +1.1, -0.9 | -0.1, +2.7 | +0.0 | 0.82x | - |
| `iw9_dm_soscar14` | scar | 0.0 / 5.3 | -0.1 / 0.8 | 0.7 | - | +8.3, +5.1 | +0.0 | 0.93x | forend +8.3 ahead, +5.1 up of the donor's left palm |
| `iw9_dm_scromeo` | svu | 0.0 / 0.9 | 0.0 / 0.0 | -0.1 | - | +10.2, +3.1 | +0.0 | 0.78x | forend +10.2 ahead, +3.1 up of the donor's left palm |
| `iw9_dm_mike14` | svu | 0.0 / 3.6 | 0.1 / 1.7 | 0.3 | +0.2, +1.6, +1.5 | +14.8, +4.9 | +0.0 | 0.88x | forend +14.8 ahead, +4.9 up of the donor's left palm |
| `iw9_dm_stango25` | svu | 0.1 / 3.5 | 0.0 / -0.5 | 1.1 | - | +7.3, +3.6 | +0.0 | 0.80x | barrel reaches 2.2 receiver lengths from its tag; barrel is 1.8x the receiver's size; forend +7.3 ahead, +3.6 up of the donor's left palm |
| `iw9_dm_pgolf1` | svu | -0.2 / -0.0 | -0.0 / 2.0 | -1.2 | +3.1, +1.3, +2.2 | +12.3, +4.7 | +0.0 | 0.93x | grip +3.1 ahead, +2.2 up of the donor's right palm; forend +12.3 ahead, +4.7 up of the donor's left palm |
| `iw9_dm_mike24` | ballista | -0.3 / 5.5 | 0.0 / 0.0 | 8.5 | - | +12.5, +4.6 | +0.0 | 0.68x | magazine hangs 9 deg off down; forend +12.5 ahead, +4.6 up of the donor's left palm |
| `iw9_sm_mpapa5` | mp7 | -1.1 / 0.1 | -3.6 / 6.4 | -0.1 | +5.0, +1.2, +2.5 | +5.7, +4.4 | +0.0 | 0.76x | bore off forward (yaw -3.6, pitch 6.4); grip +5.0 ahead, +2.5 up of the donor's right palm; forend +5.7 ahead, +4.4 up of the donor's left palm |
| `iw9_sm_mpapa7` | mp7 | -0.5 / 0.1 | 0.0 / 0.0 | 0.3 | +1.8, +1.5, +1.2 | +0.1, +3.5 | +0.0 | 0.64x | forend +0.1 ahead, +3.5 up of the donor's left palm |
| `iw9_sm_papa90` | pdw57 | -0.6 / 5.6 | -0.0 / -0.1 | -177.0 | - | -0.1, +4.6 | +0.0 | 0.88x | magazine hangs -177 deg off down; forend -0.1 ahead, +4.6 up of the donor's left palm |
| `iw9_sm_victor` | mp7 | 0.3 / 1.5 | 0.3 / 0.3 | 0.2 | - | +3.9, +2.9 | +0.0 | 0.63x | - |
| `iw9_sm_beta` | pdw57 | -0.2 / 2.0 | 0.1 / 4.0 | -2.0 | +0.0, +1.3, +2.2 | +3.2, +5.0 | +0.0 | 1.08x | forend +3.2 ahead, +5.0 up of the donor's left palm |
| `iw9_sm_alpha57` | mp7 | -0.0 / 2.7 | -0.0 / -1.6 | -179.5 | - | +4.1, +3.5 | +0.0 | 0.84x | magazine hangs -179 deg off down; forend +4.1 ahead, +3.5 up of the donor's left palm |
| `iw9_sm_mpapax` | mp7 | -0.8 / -1.9 | -2.2 / 8.1 | -0.1 | +1.2, +1.2, +1.0 | +3.4, +4.1 | +0.0 | 0.70x | bore off forward (yaw -2.2, pitch 8.1); forend +3.4 ahead, +4.1 up of the donor's left palm |
| `iw9_sm_acharlie45` | mp7 | -0.7 / -0.9 | 0.0 / 4.4 | 0.2 | +1.4, +1.2, +0.5 | +4.5, +4.1 | +0.0 | 0.74x | forend +4.5 ahead, +4.1 up of the donor's left palm |
| `iw9_sm_acharlie9` | mp7 | -0.5 / -7.5 | 5.8 / 8.7 | 0.0 | +1.4, +1.2, +0.5 | +3.2, +4.3 | +0.0 | 0.47x | gun axis off forward (yaw -0.5, pitch -7.5); bore off forward (yaw 5.8, pitch 8.7); forend +3.2 ahead, +4.3 up of the donor's left palm; length 13 against the donor's 27 |
| `iw9_sm_aviktor` | mp7 | -0.0 / 4.0 | 0.3 / 3.6 | 0.1 | +1.4, +1.7, +1.7 | +4.8, +4.5 | +0.0 | 0.80x | forend +4.8 ahead, +4.5 up of the donor's left palm |
| `iw9_sm_apapa` | mp7 | -0.5 / 8.3 | -0.0 / 0.2 | -0.3 | - | +2.3, +3.6 | +0.0 | 0.79x | gun axis off forward (yaw -0.5, pitch 8.3); forend +2.3 ahead, +3.6 up of the donor's left palm |
| `iw9_lm_rkilo` | mk48 | -0.5 / 5.0 | 2.1 / -1.4 | -0.9 | -0.1, +1.3, +5.0 | +1.8, +5.2 | +0.0 | 0.80x | grip -0.1 ahead, +5.0 up of the donor's right palm; forend +1.8 ahead, +5.2 up of the donor's left palm |
| `iw9_lm_kilo21` | mk48 | -0.0 / 0.5 | 2.1 / 0.8 | 33.2 | +3.5, +1.6, +5.7 | +1.0, +5.8 | +0.0 | 0.74x | magazine hangs 33 deg off down; grip +3.5 ahead, +5.7 up of the donor's right palm; forend +1.0 ahead, +5.8 up of the donor's left palm |
| `iw9_lm_ngolf7` | mk48 | 0.2 / 1.4 | 0.3 / 0.9 | 26.1 | - | -0.1, +4.2 | +0.0 | 0.79x | magazine hangs 26 deg off down; forend -0.1 ahead, +4.2 up of the donor's left palm |
| `iw9_sh_vecho` | saiga12 | 0.2 / 4.0 | 1.1 / -0.8 | 0.7 | -0.4, +1.3, +2.3 | -0.2, +5.5 | +0.0 | 0.58x | forend -0.2 ahead, +5.5 up of the donor's left palm; length 27 against the donor's 47 |
| `iw9_sh_mike1014` | saiga12 | -0.3 / 3.3 | -0.1 / 2.5 | 0.1 | +0.6, +1.2, +2.9 | +2.9, +5.4 | +0.0 | 0.68x | barrel reaches 2.0 receiver lengths from its tag; barrel is 1.9x the receiver's size; forend +2.9 ahead, +5.4 up of the donor's left palm |
| `iw9_sh_charlie725` | saiga12 | -0.2 / 4.1 | 0.0 / 1.9 | - | - | +3.8, +5.7 | +0.0 | 0.79x | forend +3.8 ahead, +5.7 up of the donor's left palm |
| `iw9_sh_tsierra12` | saiga12 | -0.6 / 4.6 | 1.3 / -0.4 | -2.3 | +1.1, +0.7, +0.2 | -0.1, +4.2 | +0.0 | 0.54x | forend -0.1 ahead, +4.2 up of the donor's left palm; length 25 against the donor's 47 |
| `iw9_sn_alpha50` | dsr50 | -0.3 / 0.8 | -0.1 / -0.3 | 1.6 | - | +13.9, +7.3 | +0.0 | 0.90x | barrel reaches 1.7 receiver lengths from its tag; forend +13.9 ahead, +7.3 up of the donor's left palm |
| `iw9_sn_mromeo` | dsr50 | 0.1 / 2.9 | -0.0 / 0.0 | -0.5 | +0.7, +1.1, +1.3 | +8.9, +6.2 | +0.0 | 0.72x | forend +8.9 ahead, +6.2 up of the donor's left palm |
| `iw9_sn_walpha` | svu | -0.2 / 5.2 | -0.0 / -0.2 | -0.9 | -3.4, +1.0, +1.1 | +8.2, +4.5 | +0.0 | 0.73x | grip -3.4 ahead, +1.1 up of the donor's right palm; forend +8.2 ahead, +4.5 up of the donor's left palm |
| `iw9_pi_golf17` | beretta93r | -0.2 / 24.5 | 0.0 / -3.0 | 1.1 | - | +4.7, +5.5 | +0.0 | 0.80x | gun axis off forward (yaw -0.2, pitch 24.5); forend +4.7 ahead, +5.5 up of the donor's left palm |
| `iw9_pi_decho` | beretta93r | -0.4 / 20.8 | 0.2 / -2.7 | 0.5 | +1.6, +1.2, +0.0 | +7.6, +5.2 | +0.0 | 1.05x | gun axis off forward (yaw -0.4, pitch 20.8); forend +7.6 ahead, +5.2 up of the donor's left palm |
| `iw9_pi_swhiskey` | beretta93r | -0.2 / 11.6 | 0.0 / -3.7 | -176.4 | +0.7, +1.2, -1.2 | +10.8, +5.2 | +0.0 | 1.31x | gun axis off forward (yaw -0.2, pitch 11.6); magazine hangs -176 deg off down; forend +10.8 ahead, +5.2 up of the donor's left palm |
| `iw9_pi_papa220` | beretta93r | -0.1 / 19.7 | 0.1 / -4.7 | -5.6 | +1.4, +1.1, +0.1 | +6.7, +5.3 | +0.0 | 0.79x | gun axis off forward (yaw -0.1, pitch 19.7); forend +6.7 ahead, +5.3 up of the donor's left palm |
| `iw9_pi_mike2011` | beretta93r | 0.1 / 22.3 | 0.1 / -1.8 | -1.8 | +3.0, +1.2, +0.6 | +6.8, +5.3 | +0.0 | 0.86x | gun axis off forward (yaw 0.1, pitch 22.3); grip +3.0 ahead, +0.6 up of the donor's right palm; forend +6.8 ahead, +5.3 up of the donor's left palm |
| `iw9_pi_tango9` | beretta93r | -0.6 / 2.4 | -1.7 / -3.2 | -1.3 | +1.4, +1.2, -1.1 | +12.9, +5.1 | +0.0 | 1.35x | forend +12.9 ahead, +5.1 up of the donor's left palm |

