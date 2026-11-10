//! Modern Warfare Remastered's palette light grid, as `h1_mp64_zt.exe`
//! decodes it (static analysis; `docs/H1_NATIVE.md`, "The palette light
//! grid").
//!
//! Retail maps keep no colour table: every light grid sample is a
//! palette entry, an L2 spherical harmonic (nine coefficients a colour
//! channel) packed in the grid's bitstream at the byte offset the
//! palette gives. An entry starts with a u16 of three 5-bit exponents
//! `e + 8` (red at bit 10, green at 5, blue at 0); each channel's nine
//! coefficients follow in 8, 12 or 16 bits, by its exponent against the
//! grid's two range exponents (`e <= exp8`: 8, `e <= exp12`: 12, else 16;
//! the 12 and 16-bit runs start on an even byte). With `s = 2^e`, the
//! first coefficient is `s * v / 2^bits` (16-bit for the 12-bit run) and
//! the other eight `v * 2s / 2^bits - s`.
//!
//! A model lit from one entry (a static model's colour index) gets the 56
//! shell colours of its lighting tile from it: for each shell point of a
//! 4x4x4 cube (x fastest, then y, then z; the eight inner points left
//! out; coordinates -1, -1/3, 1/3, 1, normalised to `d`), `c0 + c1 dx +
//! c2 dy + c3 dz + c4 dz dx + c5 dz dy + c6 dy dx + c7 (3 dz^2 - 1) + c8
//! (dx^2 - dy^2)`, clamped to 0..65504. A grid with no palette entries
//! lights everything with `c0 = 1`.

/// Nine coefficients (rows) of red, green, blue.
pub type Sh = [[f32; 3]; 9];

/// The shell colours of one lighting tile.
pub type ShellColours = [[f32; 3]; 56];

/// The palette of a map's light grid.
#[derive(Clone, Debug, Default)]
pub struct Palette {
    /// `rangeExponent8BitsEncoding`, `rangeExponent12BitsEncoding`,
    /// `rangeExponent16BitsEncoding` (the last is not read by the decoder).
    pub exponents: [i8; 3],
    /// Each entry's byte offset into `bits` (`paletteEntryAddress`).
    pub offsets: Vec<i32>,
    pub bits: Vec<u8>,
    /// `missingGridColorIndex`.
    pub missing: u32,
}

/// The engine's light with no palette: 1 in the first coefficient.
pub const DEFAULT_SH: Sh = {
    let mut s = [[0.0; 3]; 9];
    s[0] = [1.0, 1.0, 1.0];
    s
};

impl Palette {
    pub fn len(&self) -> usize {
        self.offsets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.offsets.is_empty()
    }

    /// Entry `i`'s coefficients; the default light when the grid has no
    /// entries, `None` when `i` or its bytes are out of range.
    pub fn sh(&self, i: u32) -> Option<Sh> {
        if self.offsets.is_empty() {
            return Some(DEFAULT_SH);
        }
        let off = usize::try_from(*self.offsets.get(i as usize)?).ok()?;
        decode_entry(self.exponents, self.bits.get(off..)?)
    }

    /// Entry `i`'s 56 shell colours.
    pub fn shell(&self, i: u32) -> Option<ShellColours> {
        self.sh(i).map(|s| shell_colours(&s))
    }
}

/// One palette entry from its bytes.
pub fn decode_entry(exponents: [i8; 3], b: &[u8]) -> Option<Sh> {
    let h = u16::from_le_bytes([*b.first()?, *b.get(1)?]);
    let e = [
        i32::from((h >> 10) & 31) - 8,
        i32::from((h >> 5) & 31) - 8,
        i32::from(h & 31) - 8,
    ];
    let mut out = [[0.0f32; 3]; 9];
    let mut at = 2usize;
    for (c, &ec) in e.iter().enumerate() {
        let s = if ec >= 0 {
            (1u32 << ec) as f32
        } else {
            1.0 / (1u32 << -ec) as f32
        };
        let neg = -s;
        if ec <= i32::from(exponents[0]) {
            // 8 bits: nine bytes.
            let v = b.get(at..at + 9)?;
            let k = 2.0 * s * (1.0 / 256.0);
            out[0][c] = s * (1.0 / 256.0) * f32::from(v[0]);
            for j in 1..9 {
                out[j][c] = f32::from(v[j]) * k + neg;
            }
            at += 9;
        } else if ec <= i32::from(exponents[1]) {
            // 12 bits: a u16, then eight 12-bit values in 12 bytes. The
            // engine's own packing: the high bytes at 2..=9, the low
            // nibbles in 9..=13 (byte 9 holds the last value's high bits,
            // which the first value's low nibble also reads).
            at += at & 1;
            let v = b.get(at..at + 14)?;
            let k = 2.0 * s * (1.0 / 4096.0);
            out[0][c] = s * (1.0 / 65536.0) * f32::from(u16::from_le_bytes([v[0], v[1]]));
            let q = |hi: u8, lo: u32| f32::from(u16::from(hi) << 4 | lo as u16);
            let w = [
                q(v[2], u32::from(v[9] >> 4)),
                q(v[3], u32::from(v[10] & 15)),
                q(v[4], u32::from(v[10] >> 4)),
                q(v[5], u32::from(v[11] & 15)),
                q(v[6], u32::from(v[11] >> 4)),
                q(v[7], u32::from(v[12] & 15)),
                q(v[8], u32::from(v[12] >> 4)),
                q(v[9], u32::from(v[13] & 15)),
            ];
            for j in 1..9 {
                out[j][c] = w[j - 1] * k + neg;
            }
            at += 14;
        } else {
            // 16 bits: nine u16.
            at += at & 1;
            let v = b.get(at..at + 18)?;
            let u = |j: usize| f32::from(u16::from_le_bytes([v[2 * j], v[2 * j + 1]]));
            let k = 2.0 * s * (1.0 / 65536.0);
            out[0][c] = s * (1.0 / 65536.0) * u(0);
            for j in 1..9 {
                out[j][c] = u(j) * k + neg;
            }
            at += 18;
        }
    }
    Some(out)
}

/// The 56 shell directions (x fastest, then y, then z; inner points left
/// out), normalised.
pub fn shell_directions() -> [[f32; 3]; 56] {
    let mut d = [[0.0f32; 3]; 56];
    let mut n = 0;
    let step = 2.0f32 / 3.0;
    for z in 0..4i32 {
        for y in 0..4i32 {
            for x in 0..4i32 {
                let inner = (1..=2).contains(&x) && (1..=2).contains(&y) && (1..=2).contains(&z);
                if inner {
                    continue;
                }
                let p = [
                    x as f32 * step - 1.0,
                    y as f32 * step - 1.0,
                    z as f32 * step - 1.0,
                ];
                let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
                d[n] = [p[0] / l, p[1] / l, p[2] / l];
                n += 1;
            }
        }
    }
    d
}

/// The 56 shell colours of a harmonic.
pub fn shell_colours(sh: &Sh) -> ShellColours {
    let dirs = shell_directions();
    let mut out = [[0.0f32; 3]; 56];
    for (o, d) in out.iter_mut().zip(dirs) {
        let [x, y, z] = d;
        let basis = [
            x,
            y,
            z,
            z * x,
            z * y,
            y * x,
            3.0 * z * z - 1.0,
            x * x - y * y,
        ];
        for c in 0..3 {
            let mut v = sh[0][c];
            for (k, b) in basis.iter().enumerate() {
                v += sh[k + 1][c] * b;
            }
            o[c] = v.clamp(0.0, 65504.0);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(e: [i32; 3]) -> [u8; 2] {
        let h = ((e[0] + 8) << 10 | (e[1] + 8) << 5 | (e[2] + 8)) as u16;
        h.to_le_bytes()
    }

    #[test]
    fn eight_bit_channels() {
        // All three channels at e = 0 (s = 1) with exp8 = 0: 8 bits.
        let mut b = header([0, 0, 0]).to_vec();
        for _ in 0..3 {
            b.push(128); // c0 = 0.5
            b.extend([128u8; 8]); // ck = 128 * 2/256 - 1 = 0
        }
        let sh = decode_entry([0, 4, 8], &b).unwrap();
        assert_eq!(sh[0], [0.5; 3]);
        for row in &sh[1..] {
            assert_eq!(*row, [0.0; 3]);
        }
    }

    #[test]
    fn widths_follow_the_exponents() {
        // Red 8-bit (e -1), green 12-bit (e 2), blue 16-bit (e 5).
        let mut b = header([-1, 2, 5]).to_vec();
        b.push(255);
        b.extend([0u8; 8]);
        // at = 11: odd, the 12-bit run starts at 12.
        b.push(0);
        let mut g = [0u8; 14];
        g[0..2].copy_from_slice(&32768u16.to_le_bytes());
        b.extend(g);
        // at = 26: even.
        let mut bl = Vec::new();
        for j in 0..9u16 {
            bl.extend((if j == 0 { 65535 } else { 32768u16 }).to_le_bytes());
        }
        b.extend(bl);
        let sh = decode_entry([0, 3, 8], &b).unwrap();
        // Red: s = 0.5, c0 = 0.5 * 255/256, ck = 0 * 1/256 - 0.5.
        assert!((sh[0][0] - 0.5 * 255.0 / 256.0).abs() < 1e-6);
        assert!((sh[1][0] + 0.5).abs() < 1e-6);
        // Green: s = 4, c0 = 4 * 0.5, ck = 0 - 4.
        assert!((sh[0][1] - 2.0).abs() < 1e-6);
        assert!((sh[5][1] + 4.0).abs() < 1e-6);
        // Blue: s = 32, c0 = 32 * 65535/65536, ck = 32768 * 64/65536 - 32 = 0.
        assert!((sh[0][2] - 32.0 * 65535.0 / 65536.0).abs() < 1e-3);
        assert!(sh[8][2].abs() < 1e-6);
    }

    #[test]
    fn twelve_bit_packing_as_the_engine_reads_it() {
        let mut b = header([1, 1, 1]).to_vec();
        let mut v = [0u8; 14];
        v[2] = 0x12;
        v[9] = 0xab; // c8's high byte; c1's low nibble reads 0xa
        v[13] = 0x0c;
        b.extend(v);
        b.extend(v);
        b.extend(v);
        let sh = decode_entry([0, 4, 8], &b).unwrap();
        let k = 2.0 * 2.0 / 4096.0;
        assert!((sh[1][0] - (f32::from(0x12au16) * k - 2.0)).abs() < 1e-5);
        assert!((sh[8][0] - (f32::from(0xabcu16) * k - 2.0)).abs() < 1e-5);
    }

    #[test]
    fn shell_order_and_basis() {
        let d = shell_directions();
        let r3 = 1.0 / 3f32.sqrt();
        assert!((d[0][0] + r3).abs() < 1e-6 && (d[0][2] + r3).abs() < 1e-6);
        assert!((d[55][0] - r3).abs() < 1e-6 && (d[55][2] - r3).abs() < 1e-6);
        // A constant harmonic is the same in every direction; an x-only one
        // is brighter on +x than -x and clamped at 0 below.
        let mut sh = DEFAULT_SH;
        let flat = shell_colours(&sh);
        assert!(flat.iter().all(|c| *c == [1.0; 3]));
        sh[1] = [2.0, 0.0, 0.0];
        let c = shell_colours(&sh);
        assert!(c[3][0] > c[0][0]);
        assert_eq!(c[0][0], 0.0);
    }

    #[test]
    fn empty_palette_is_the_default_light() {
        let p = Palette::default();
        assert_eq!(p.sh(7), Some(DEFAULT_SH));
    }
}
