//! bo2mp: a run of words with no spaces split into its words
//! (`rgbwarrior` -> `RGB Warrior`), for names the games keep as one token:
//! Modern Warfare III's calling cards and emblems, whose titles are not
//! read (`jup_cards`). The most likely split by how common each word is in
//! English (`assets/words/words_en.txt`: the 40,000 most common words,
//! most common first, from the `wordsegment` package's unigram counts,
//! Apache 2.0; see NOTICE).

use std::collections::HashMap;
use std::sync::OnceLock;

const WORDS: &str = include_str!("../assets/words/words_en.txt");

/// Words of the games kept whole or said their own way (a split would part
/// them, or the list has them lower).
const GAME_WORDS: &[(&str, &str)] = &[
    ("warzone", "Warzone"),
    ("dmz", "DMZ"),
    ("exfil", "Exfil"),
    ("infil", "Infil"),
    ("killstreak", "Killstreak"),
    ("scorestreak", "Scorestreak"),
    ("cod", "CoD"),
    ("mw", "MW"),
    ("mwii", "MWII"),
    ("mwiii", "MWIII"),
    ("uav", "UAV"),
    ("emp", "EMP"),
    ("xp", "XP"),
    ("opfor", "OpFor"),
    ("ghillie", "Ghillie"),
    ("zombies", "Zombies"),
];

/// The longest word looked for.
const MAX_WORD: usize = 20;

fn ranks() -> &'static HashMap<&'static str, usize> {
    static R: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
    R.get_or_init(|| {
        let mut m: HashMap<&'static str, usize> = WORDS.lines().filter(|w| !w.is_empty()).enumerate().map(|(i, w)| (w, i)).collect();
        // (The games' own words as if common.)
        for (w, _) in GAME_WORDS {
            m.insert(w, 0);
        }
        m
    })
}

/// What a piece costs as one word: a known word by how rare it is, an
/// unknown one by its length (so a short unknown run stays whole rather
/// than breaking into rare scraps).
fn cost(piece: &str) -> f64 {
    // (A two-letter piece with a vowel is rarely a word of a name - `pa`,
    // `ap` - unless it is all there is to break off; DJ, TV are kept.)
    let short = if piece.len() == 2 && piece.chars().any(|c| "aeiouy".contains(c)) { 4.0 } else { 0.0 };
    match ranks().get(piece) {
        Some(&r) => ((r + 2) as f64).ln() + 3.0 + short,
        None => 12.0 + 2.5 * piece.len() as f64,
    }
}

/// `s` (lowercase letters) as its most likely words.
pub(crate) fn split(s: &str) -> Vec<String> {
    let s = s.to_ascii_lowercase();
    let n = s.len();
    // (A short name stays whole: `apapa`, `katana`.)
    if n <= 5 || !s.is_ascii() {
        return vec![s];
    }
    let mut best = vec![f64::INFINITY; n + 1];
    let mut back = vec![0usize; n + 1];
    best[0] = 0.0;
    for i in 1..=n {
        for j in i.saturating_sub(MAX_WORD)..i {
            // (No one-letter pieces: `apapa` is a name, not `A Papa`.)
            if i - j == 1 && n > 1 {
                continue;
            }
            let c = best[j] + cost(&s[j..i]);
            if c < best[i] {
                best[i] = c;
                back[i] = j;
            }
        }
    }
    let mut out = Vec::new();
    let mut i = n;
    while i > 0 {
        out.push(s[back[i]..i].to_owned());
        i = back[i];
    }
    out.reverse();
    out
}

/// A word as a title shows it: the games' own spelling, a short word with
/// no vowel in capitals (RGB, DJ, TV), else capitalised.
pub(crate) fn title_word(w: &str) -> String {
    if let Some((_, nice)) = GAME_WORDS.iter().find(|(k, _)| *k == w) {
        return (*nice).to_owned();
    }
    if w.len() <= 4 && w.chars().all(|c| c.is_ascii_alphabetic()) && !w.chars().any(|c| "aeiouy".contains(c)) {
        return w.to_ascii_uppercase();
    }
    let mut c = w.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// A token as words in title case (`rgbwarrior` -> `RGB Warrior`; digits
/// stay as they are).
pub(crate) fn title_token(token: &str) -> String {
    if token.chars().all(|c| c.is_ascii_digit()) || !token.chars().all(|c| c.is_ascii_alphabetic()) {
        return title_word(token);
    }
    split(token).iter().map(|w| title_word(w)).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joined_words_part() {
        assert_eq!(title_token("rgbwarrior"), "RGB Warrior");
        assert_eq!(title_token("greekmythhades"), "Greek Myth Hades");
        assert_eq!(title_token("neonnights"), "Neon Nights");
        assert_eq!(title_token("djbooth"), "DJ Booth");
        assert_eq!(title_token("warzone"), "Warzone");
        assert_eq!(title_token("katana"), "Katana");
        assert_eq!(title_token("06"), "06");
        assert_eq!(title_token("apapa"), "Apapa");
        assert_eq!(title_token("lilbuddy"), "Lil Buddy");
        assert_eq!(title_token("goldenrooster"), "Golden Rooster");
    }
}
