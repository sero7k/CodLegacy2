//! bo2mp: the public pre-game lobby's map vote. Two maps from the
//! playlist, never one map twice in two games' versions (Nuketown and
//! Nuketown 2025, Call of Duty 4's Crash and Modern Warfare 2's), nor the
//! map just played; the most votes win, a tie is a coin toss.

/// A map's identity across games: the same map in two games' versions
/// gives the same identity. From its title when there is one (`Nuketown
/// 2025` -> `nuketown`), else from its zone name (`t5:mp_nuked` ->
/// `nuketown`).
pub fn identity(map: &str, title: Option<&str>) -> String {
    let name = match title.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => t.split('(').next().unwrap_or(t).to_owned(),
        None => {
            let stem = map.rsplit(':').next().unwrap_or(map).to_ascii_lowercase();
            let stem = stem.strip_prefix("mp_").unwrap_or(&stem).to_owned();
            stem.split('_')
                .filter(|w| !SUFFIXES.contains(w))
                .collect::<Vec<_>>()
                .join("")
        }
    };
    let mut key: String = name
        .to_ascii_lowercase()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    for word in ["remastered", "classic", "night"] {
        if let Some(k) = key.strip_suffix(word).filter(|k| !k.is_empty()) {
            key = k.to_owned();
        }
    }
    let key = key.trim_end_matches(|c: char| c.is_ascii_digit()).to_owned();
    ALIASES
        .iter()
        .find(|(a, _)| *a == key)
        .map_or(key, |(_, b)| (*b).to_owned())
}

/// Zone-name words that mark a version, not a map (`mp_terminal_cls`,
/// `mp_shipment_iw3`).
const SUFFIXES: &[&str] = &["cls", "iw3", "iw4", "iw5", "t5", "t6", "ns", "dlc", "mp", "remastered", "2020"];

/// Zone names whose map is known by another name.
const ALIASES: &[(&str, &str)] = &[
    ("nuked", "nuketown"),
    ("mountain", "summit"),
    ("boneyard", "scrapyard"),
    ("checkpoint", "karachi"),
    ("brecourt", "wasteland"),
    ("abandon", "carnival"),
    ("complex", "bailout"),
    ("compact", "salvage"),
];

/// Two maps to vote between, from `pool` (map, title): different maps
/// (by `identity`), neither the one just played; fewer when the pool has
/// fewer. `seed`: any number (the draw's dice).
pub fn pick(pool: &[(String, Option<String>)], played: Option<&str>, seed: u64) -> Vec<String> {
    let played = played.map(|p| {
        let stem = |m: &str| m.rsplit(':').next().unwrap_or(m).to_ascii_lowercase();
        let title = pool.iter().find(|(m, _)| stem(m) == stem(p)).and_then(|(_, t)| t.as_deref());
        identity(p, title)
    });
    let mut dice = Dice(seed | 1);
    let mut out: Vec<String> = Vec::new();
    let mut taken: Vec<String> = played.clone().into_iter().collect();
    for _ in 0..2 {
        let left: Vec<&(String, Option<String>)> = pool
            .iter()
            .filter(|(m, t)| !taken.contains(&identity(m, t.as_deref())))
            .collect();
        let Some((m, t)) = (!left.is_empty()).then(|| left[dice.next() as usize % left.len()]) else {
            break;
        };
        taken.push(identity(m, t.as_deref()));
        out.push(m.clone());
    }
    // (A pool of one map: that map again.)
    if out.is_empty()
        && let Some((m, _)) = pool.first()
    {
        out.push(m.clone());
    }
    out
}

/// The vote's winner: the most votes; a tie (no votes: everyone ties) a
/// coin toss among the tied.
pub fn winner(maps: &[String], counts: &[u8], seed: u64) -> Option<String> {
    let best = (0..maps.len()).map(|i| counts.get(i).copied().unwrap_or(0)).max()?;
    let tied: Vec<usize> = (0..maps.len()).filter(|&i| counts.get(i).copied().unwrap_or(0) == best).collect();
    let k = tied[Dice(seed | 1).next() as usize % tied.len()];
    maps.get(k).cloned()
}

struct Dice(u64);

impl Dice {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}
