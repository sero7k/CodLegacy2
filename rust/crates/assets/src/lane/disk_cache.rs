//! bo2mp: the game's compressed disk cache entries: a binary writer and
//! reader, and the packed file every cached conversion is kept in (lz4 in
//! 8 MiB pieces behind a magic, a format and the key's hash). Shared by the
//! map caches (`jup_cache`, `rt_cache`). The magic is kept as it was first
//! written, so entries already on disk stay valid.

pub(super) const FORMAT: u32 = 1;
const MAGIC: &[u8; 8] = b"BO2JUPM\0";

pub(super) fn fnv(mut h: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

pub(super) const FNV_START: u64 = 0xcbf2_9ce4_8422_2325;

#[derive(Default)]
pub(super) struct W(pub(super) Vec<u8>);

impl W {
    pub(super) fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    pub(super) fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub(super) fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub(super) fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub(super) fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub(super) fn len(&mut self, n: usize) {
        self.u64(n as u64);
    }
    pub(super) fn f2(&mut self, v: [f32; 2]) {
        v.iter().for_each(|&x| self.f32(x));
    }
    pub(super) fn f3(&mut self, v: [f32; 3]) {
        v.iter().for_each(|&x| self.f32(x));
    }
    pub(super) fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }
    pub(super) fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
    pub(super) fn bytes(&mut self, b: &[u8]) {
        self.len(b.len());
        self.0.extend_from_slice(b);
    }
    pub(super) fn opt_index(&mut self, v: Option<usize>) {
        self.u64(v.map_or(u64::MAX, |v| v as u64));
    }
    pub(super) fn f3s(&mut self, v: &[[f32; 3]]) {
        self.len(v.len());
        self.0.reserve(v.len() * 12);
        v.iter().for_each(|&p| self.f3(p));
    }
    pub(super) fn f2s(&mut self, v: &[[f32; 2]]) {
        self.len(v.len());
        v.iter().for_each(|&p| self.f2(p));
    }
    pub(super) fn tris16(&mut self, v: &[[u16; 3]]) {
        self.len(v.len());
        v.iter().flatten().for_each(|&i| self.u16(i));
    }
    pub(super) fn bools(&mut self, v: &[bool]) {
        self.len(v.len());
        v.iter().for_each(|&b| self.bool(b));
    }
}

// ---------------------------------------------------------------- reading

pub(super) struct R<'a> {
    pub(super) b: &'a [u8],
    pub(super) at: usize,
}

impl<'a> R<'a> {
    pub(super) fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let s = self.b.get(self.at..end)?;
        self.at = end;
        Some(s)
    }
    pub(super) fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    pub(super) fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    pub(super) fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    pub(super) fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    pub(super) fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    /// A count of items of at least `each` bytes (never more than the bytes
    /// left could hold).
    pub(super) fn len(&mut self, each: usize) -> Option<usize> {
        let n = usize::try_from(self.u64()?).ok()?;
        (n.checked_mul(each.max(1))? <= self.b.len() - self.at).then_some(n)
    }
    pub(super) fn f2(&mut self) -> Option<[f32; 2]> {
        Some([self.f32()?, self.f32()?])
    }
    pub(super) fn f3(&mut self) -> Option<[f32; 3]> {
        Some([self.f32()?, self.f32()?, self.f32()?])
    }
    pub(super) fn bool(&mut self) -> Option<bool> {
        Some(self.u8()? != 0)
    }
    pub(super) fn bytes(&mut self) -> Option<Vec<u8>> {
        let n = self.len(1)?;
        Some(self.take(n)?.to_vec())
    }
    pub(super) fn str(&mut self) -> Option<String> {
        String::from_utf8(self.bytes()?).ok()
    }
    pub(super) fn opt_index(&mut self) -> Option<Option<usize>> {
        let v = self.u64()?;
        Some((v != u64::MAX).then_some(v as usize))
    }
    pub(super) fn vec<T>(&mut self, each: usize, mut item: impl FnMut(&mut Self) -> Option<T>) -> Option<Vec<T>> {
        let n = self.len(each)?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(item(self)?);
        }
        Some(out)
    }
    pub(super) fn f3s(&mut self) -> Option<Vec<[f32; 3]>> {
        self.vec(12, Self::f3)
    }
    pub(super) fn f2s(&mut self) -> Option<Vec<[f32; 2]>> {
        self.vec(8, Self::f2)
    }
    pub(super) fn tris16(&mut self) -> Option<Vec<[u16; 3]>> {
        self.vec(6, |r| Some([r.u16()?, r.u16()?, r.u16()?]))
    }
    pub(super) fn bools(&mut self) -> Option<Vec<bool>> {
        self.vec(1, Self::bool)
    }
}

// ------------------------------------------------------------ the records

// ----------------------------------------------------------- the files

/// Bytes compressed in pieces (several at once), behind the magic, the
/// format and the key.
pub(super) fn pack(key: &str, raw: &[u8]) -> Vec<u8> {
    const PIECE: usize = 8 << 20;
    let pieces: Vec<&[u8]> = raw.chunks(PIECE).collect();
    let packed: Vec<Vec<u8>> = std::thread::scope(|s| {
        let jobs: Vec<_> = pieces.iter().map(|p| s.spawn(move || lz4_flex::block::compress(p))).collect();
        jobs.into_iter().map(|j| j.join().unwrap_or_default()).collect()
    });
    let mut out = Vec::with_capacity(packed.iter().map(Vec::len).sum::<usize>() + 64 + 16 * pieces.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&FORMAT.to_le_bytes());
    out.extend_from_slice(&fnv(FNV_START, key.as_bytes()).to_le_bytes());
    out.extend_from_slice(&(pieces.len() as u64).to_le_bytes());
    for (p, c) in pieces.iter().zip(&packed) {
        out.extend_from_slice(&(p.len() as u64).to_le_bytes());
        out.extend_from_slice(&(c.len() as u64).to_le_bytes());
    }
    for c in &packed {
        out.extend_from_slice(c);
    }
    out
}

/// What [`pack`] made, unpacked (several pieces at once); `None` when it
/// is not whole or is another key's.
pub(super) fn unpack(key: &str, b: &[u8]) -> Option<Vec<u8>> {
    let mut r = R { b, at: 0 };
    if r.take(8)? != MAGIC || r.u32()? != FORMAT || r.u64()? != fnv(FNV_START, key.as_bytes()) {
        return None;
    }
    let n = r.len(16)?;
    let sizes: Vec<(usize, usize)> =
        (0..n).map(|_| Some((usize::try_from(r.u64()?).ok()?, usize::try_from(r.u64()?).ok()?))).collect::<Option<_>>()?;
    let mut pieces = Vec::with_capacity(n);
    for &(raw, packed) in &sizes {
        pieces.push((raw, r.take(packed)?));
    }
    if r.at != b.len() {
        return None;
    }
    let total: usize = sizes.iter().map(|s| s.0).sum();
    let mut out = vec![0u8; total];
    let ok = std::thread::scope(|s| {
        let mut rest = out.as_mut_slice();
        let mut jobs = Vec::new();
        for &(raw, packed) in &pieces {
            let (here, after) = rest.split_at_mut(raw);
            rest = after;
            jobs.push(s.spawn(move || lz4_flex::block::decompress_into(packed, here).is_ok_and(|n| n == raw)));
        }
        jobs.into_iter().all(|j| j.join().unwrap_or(false))
    });
    ok.then_some(out)
}

/// Reads a cache entry and unpacks it.
pub(super) fn read(kind: &str, key: &str) -> Option<Vec<u8>> {
    let bytes = asset_transport::cache_get(kind, key)?;
    unpack(key, &bytes)
}

/// Writes a cache entry (the map's other entries of this kind removed),
/// on a thread of its own: `encode` runs there.
pub(super) fn write_later(kind: &'static str, key: String, encode: impl FnOnce() -> Vec<u8> + Send + 'static) {
    let spawned = std::thread::Builder::new().name(format!("{kind} cache write")).spawn(move || {
        let started = std::time::Instant::now();
        let raw = encode();
        let packed = pack(&key, &raw);
        let map = key.rsplit_once('-').map_or(key.as_str(), |(m, _)| m);
        asset_transport::cache_prune(kind, &format!("{map}-"), &key);
        match asset_transport::cache_put(kind, &key, &packed) {
            Ok(()) => diag::info!(
                World,
                "bo2mp cache: {kind}/{key} written, {:.1} MiB ({:.1} MiB unpacked) in {:.1}s",
                packed.len() as f64 / (1024.0 * 1024.0),
                raw.len() as f64 / (1024.0 * 1024.0),
                started.elapsed().as_secs_f32()
            ),
            Err(e) => diag::warn!(World, "bo2mp cache: {kind}/{key} not written: {e}"),
        }
    });
    if let Err(e) = spawned {
        diag::warn!(World, "bo2mp cache: {kind} not written (no thread: {e})");
    }
}

