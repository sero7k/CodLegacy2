//! bo2mp: the parts of an inflated zone something still reads once the map
//! is up, copied out so the zone itself can be let go (Skyjacked's is
//! hundreds of MB, held for the whole match only for a few KB of fog,
//! vision and exposure records and the pictures kept in the zone).
//!
//! The spans are merged where they meet or overlap and laid end to end in
//! their old order; a reader takes [`KeptBytes::bytes`] with its offsets
//! moved by [`KeptBytes::rebase`]. Read inside the spans it was given, it
//! sees the same bytes as in the whole zone.

/// An offset that was in no kept span: far past the bytes, so a read there
/// misses as a read past the zone's end does (and adding a record's size
/// to it cannot overflow).
pub const MISSING: usize = usize::MAX / 4;

#[derive(Clone, Debug, Default)]
pub struct KeptBytes {
    bytes: Vec<u8>,
    /// (old start, old end, new start), in old order.
    chunks: Vec<(usize, usize, usize)>,
}

impl KeptBytes {
    /// The spans `(offset, length)` of `image` (cut to its end; an empty
    /// one keeps nothing).
    pub fn new(image: &[u8], spans: impl IntoIterator<Item = (usize, usize)>) -> Self {
        let mut spans: Vec<(usize, usize)> = spans
            .into_iter()
            .filter_map(|(at, len)| {
                let end = at.saturating_add(len).min(image.len());
                (at < end).then_some((at, end))
            })
            .collect();
        spans.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
        for (at, end) in spans {
            match merged.last_mut() {
                Some(last) if at <= last.1 => last.1 = last.1.max(end),
                _ => merged.push((at, end)),
            }
        }
        let mut bytes = Vec::with_capacity(merged.iter().map(|(a, e)| e - a).sum());
        let chunks = merged
            .into_iter()
            .map(|(at, end)| {
                let new = bytes.len();
                bytes.extend_from_slice(&image[at..end]);
                (at, end, new)
            })
            .collect();
        Self { bytes, chunks }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Where old offset `at` is now (a span's end counts as in it, for an
    /// empty array kept at another's end), or [`MISSING`].
    pub fn rebase(&self, at: usize) -> usize {
        let i = self.chunks.partition_point(|c| c.0 <= at);
        match i.checked_sub(1).map(|i| self.chunks[i]) {
            Some((start, end, new)) if at <= end => new + (at - start),
            _ => MISSING,
        }
    }

    /// A walk span `(offset, count)` with its offset rebased.
    pub fn span(&self, s: Option<(usize, usize)>) -> Option<(usize, usize)> {
        s.map(|(at, n)| (self.rebase(at), n))
    }

    /// The `len` bytes at old offset `at`, when one kept span holds them all.
    pub fn get(&self, at: usize, len: usize) -> Option<&[u8]> {
        let new = self.rebase(at);
        let i = self.chunks.partition_point(|c| c.0 <= at).checked_sub(1)?;
        (at.checked_add(len)? <= self.chunks[i].1).then(|| &self.bytes[new..new + len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kept_spans_read_as_the_whole_image() {
        let image: Vec<u8> = (0..4096u32).map(|i| (i * 7 + i / 13) as u8).collect();
        let spans = [(100, 40), (130, 50), (2000, 16), (4090, 64), (3000, 0), (180, 4)];
        let k = KeptBytes::new(&image, spans);
        // 100..184 merged (they overlap and meet), 2000..2016, 4090..4096.
        assert_eq!(k.len(), 84 + 16 + 6);
        for (at, len) in spans.into_iter().filter(|s| s.1 > 0) {
            let end = (at + len).min(image.len());
            for o in at..end {
                assert_eq!(k.bytes()[k.rebase(o)], image[o], "offset {o}");
            }
            assert_eq!(k.get(at, end - at), image.get(at..end));
        }
        // A read across two kept spans or outside them misses.
        assert_eq!(k.get(170, 1900), None);
        assert_eq!(k.rebase(50), MISSING);
        assert_eq!(k.rebase(3000), MISSING);
        assert!(k.bytes().get(k.rebase(50)..k.rebase(50) + 4).is_none());
        // The end of a span rebases (an empty array kept at its end).
        assert_eq!(k.rebase(184), 84);
        assert_eq!(k.span(Some((2000, 1))), Some((84, 1)));
    }
}
