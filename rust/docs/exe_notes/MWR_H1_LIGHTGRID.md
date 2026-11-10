# Private notes: MWR light grid (31 Oct 2026). Addresses: keep out of the repo.

Tools on this PC: JDK `C:\Users\Gaming PC\tools\jdk-21.0.13+9`, Ghidra `C:\Users\Gaming PC\tools\ghidra_11.4.2_PUBLIC`,
project `C:\Users\Gaming PC\tools\ghidra_proj` (name h1, program h1_mp64_zt.exe), scripts `C:\Users\Gaming PC\tools\ghidra_scripts`.
Dec.java patched locally: range/bytes args take `_` as separator too (cmd splits on commas). Hex args need `0x`.

- rgp.world pointer: DAT_14fd70160.
- Light grid "dirty"/sky entry writers: FUN_140630290 (encodes palette entry 1 from a 0x90 SH block via FUN_1405f7080 using
  world+0x228/0x229/0x22a exponents), FUN_140630380, FUN_140630400 (copy 0x38 bytes at bitstream + palette[1]). Gate world+0x238 > 1.
- Palette entry address: bitstream(world+0x250) + (int)paletteEntryAddress(world+0x240)[i].
- Entry decode FUN_1405f6d30(exps, src u16*, out 0x90): header u16 -> e_c = ((h>>10)&31)-8, ((h>>5)&31)-8, (h&31)-8 for c=0,1,2;
  scale = 2^e (1/2^-e when negative); width 8 if e<=exp8, else 12 if e<=exp12, else 16. Offset starts 2; 12/16 align even.
  - 8-bit FUN_1405f5940: 9 bytes; c0 = s*K8*b0; ck = bk*(2 s K8) - s; K8 = DAT_140831ff4 (1/255?). returns 9.
  - 12-bit FUN_1405f56a0: 14 bytes; c0 = s*K16*u16[0]; others 12-bit (see decompile); K12 = DAT_1408177e4. returns 14.
  - 16-bit FUN_1405f5820: 9 u16; K16 = DAT_1408177dc. returns 18.
  - Output: out[row k*4 + channel c] floats, rows 0..8 (L2 SH, 9 coefficients), channel c.
- Callers of the decoder: 14062c894, 14062d169, 14062d253, 14062d521, 14062d5da, 14062fdd7.
- Bit reader FUN_1405f5450(reader, nbits) LSB-first; reader+0x10 bit pos u64, +0x18 data ptr, +4 byte pos.
- Constants: K8 1/256 (DAT_140831ff4), K12 1/4096 (DAT_1408177e4), K16 1/65536 (DAT_1408177dc); grid 1/32 (DAT_140812ffc),
  1/64 z (DAT_14081307c), origin -131072 (DAT_140813398), max 131072 (DAT_140813378); clamp 0..65504 (DAT_140867ee0/140890c50);
  default SH c0=(1,1,1,0) at DAT_140859f60. Basis A=3 (DAT_140890c40), B=1 (DAT_140867f00), step 2/3 (DAT_140813084).
- 12-bit encoder FUN_1405f5a20 (byte 9 overwrite quirk confirmed); 8-bit enc FUN_1405f5d90; 16-bit enc FUN_1405f5c00; entry enc FUN_1405f7080.
- Shell directions built by FUN_1405f6140 into DAT_14fead630 (56 vec4), basis table FUN_1405f6260 into DAT_14fead9b0 (56 x 8 vec4).
  Init caller FUN_1405feac0. SH -> 56 half3: FUN_1405f8660 (half convert FUN_1404f0790).
- Light grid struct = world + 0x1b0 (exps +0x78, missing idx +0x6c, palette count +0x88, palette +0x90, bitstream +0xa0).
- Model light at a point: FUN_14062d2f0 (lookup FUN_14062bdc0 -> corner indices/weights/sun vis; normalise; blend; contrast dvars
  DAT_1415bc750/758/760 + DAT_14fd61c58/c60; FUN_14062cf00 sun; FUN_1405f8660). Single entry: FUN_14062fd20. Blend list: FUN_14062c7a0.
  Trilinear weights: FUN_14062cac0 / FUN_14062cbd0. Big grid fn FUN_14062d850 (not read yet).
- FUN_1405f6ea0: tree mask decode (2-bit mode: 0 -> 16 bits raw; 1 -> 0xffff; 2/3 -> 4-bit table index into DAT_14fe9c428 (0x1c stride), 3 inverts).
