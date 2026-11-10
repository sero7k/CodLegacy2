//! bo2mp: Black Ops III's community maps: the Steam Workshop items he has
//! subscribed to and the maps in BO3's own `usermaps` folder.
//!
//! Where they are:
//! - Steam keeps each Workshop item in
//!   `<library>/steamapps/workshop/content/311210/<publisher id>/` (311210
//!   is Black Ops III's app id), in the library BO3 is installed in (found
//!   from BO3's folder, `<library>/steamapps/common/<game>`) or any other
//!   Steam library (`game_scan::steam_libraries`, which reads
//!   `libraryfolders.vdf`). `CODLEGACY_BO3_WORKSHOP` adds more such folders
//!   (`;`-separated); `CODLEGACY_BO3_WORKSHOP=0` turns the search off.
//! - The mod tools' own maps: `<bo3>/usermaps/<map>/zone/<map>.ff`.
//!
//! Each item folder holds `workshop.json` (`Title`, `FolderName`, `Type`,
//! `Tags`, `PublisherID`, `Description`), the map's zone
//! `<FolderName>.ff` with its streamed data `<FolderName>.xpak` beside it,
//! language stubs (`en_<name>.ff`, ...) and its pictures
//! (`loadingimage.png`, `previewimage.png`, `workshop.jpg`). Items whose
//! `Type` is `mod` are kept apart (`Discovery::mods`: weapon packs, whose
//! guns `assets::lane::t7_packs` reads); other kinds are left out, as are
//! maps whose zone is missing. The map items' folders are searched for zones with the games'
//! (`discover::search_roots`), so a Workshop map lists and loads as one of
//! BO3's own.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Black Ops III's Steam app id (its Workshop content folder's name).
pub const BO3_APP_ID: &str = "311210";

/// What an item's `workshop.json` says (its string members).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkshopJson {
    pub title: String,
    pub folder_name: String,
    /// `map` or `mod`.
    pub kind: String,
    pub tags: String,
    pub publisher_id: String,
    pub description: String,
}

/// Reads a `workshop.json`. Only its top-level string members are kept;
/// other values are skipped. Keys match without regard to case.
pub fn parse_workshop_json(text: &str) -> Result<WorkshopJson, String> {
    let members = json_object_strings(text)?;
    let get = |key: &str| members.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.clone()).unwrap_or_default();
    Ok(WorkshopJson {
        title: get("Title"),
        folder_name: get("FolderName"),
        kind: get("Type"),
        tags: get("Tags"),
        publisher_id: get("PublisherID"),
        description: get("Description"),
    })
}

/// The string members of a JSON object (`{"k": "v", ...}`), in order.
/// Members of other kinds (numbers, arrays, objects, ...) are parsed and
/// skipped.
fn json_object_strings(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut p = Json { b: text.trim_start_matches('\u{feff}').as_bytes(), at: 0 };
    p.ws();
    p.eat(b'{')?;
    let mut out = Vec::new();
    p.ws();
    if p.peek() == Some(b'}') {
        return Ok(out);
    }
    loop {
        p.ws();
        let key = p.string()?;
        p.ws();
        p.eat(b':')?;
        p.ws();
        if p.peek() == Some(b'"') {
            out.push((key, p.string()?));
        } else {
            p.skip_value(0)?;
        }
        p.ws();
        match p.next() {
            Some(b',') => continue,
            Some(b'}') => return Ok(out),
            other => return Err(format!("expected , or }} at {}, found {:?}", p.at, other.map(char::from))),
        }
    }
}

struct Json<'a> {
    b: &'a [u8],
    at: usize,
}

impl Json<'_> {
    fn peek(&self) -> Option<u8> {
        self.b.get(self.at).copied()
    }
    fn next(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.at += 1;
        Some(c)
    }
    fn ws(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
            self.at += 1;
        }
    }
    fn eat(&mut self, want: u8) -> Result<(), String> {
        match self.next() {
            Some(c) if c == want => Ok(()),
            other => Err(format!("expected {} at {}, found {:?}", char::from(want), self.at, other.map(char::from))),
        }
    }
    fn hex4(&mut self) -> Result<u32, String> {
        let s = self.b.get(self.at..self.at + 4).ok_or("short \\u escape")?;
        self.at += 4;
        u32::from_str_radix(std::str::from_utf8(s).map_err(|e| e.to_string())?, 16).map_err(|e| format!("bad \\u escape: {e}"))
    }
    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            match self.next().ok_or("unterminated string")? {
                b'"' => return String::from_utf8(out).map_err(|e| e.to_string()),
                b'\\' => match self.next().ok_or("unterminated escape")? {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(8),
                    b'f' => out.push(12),
                    b'u' => {
                        let mut cp = self.hex4()?;
                        // (A surrogate pair.)
                        if (0xD800..0xDC00).contains(&cp) && self.b.get(self.at..self.at + 2) == Some(b"\\u") {
                            self.at += 2;
                            let lo = self.hex4()?;
                            cp = 0x10000 + ((cp - 0xD800) << 10) + (lo.wrapping_sub(0xDC00) & 0x3FF);
                        }
                        let ch = char::from_u32(cp).unwrap_or('\u{fffd}');
                        out.extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes());
                    }
                    c => out.push(c),
                },
                c => out.push(c),
            }
        }
    }
    fn skip_value(&mut self, depth: usize) -> Result<(), String> {
        if depth > 64 {
            return Err("nested too deep".into());
        }
        self.ws();
        match self.peek().ok_or("value missing")? {
            b'"' => self.string().map(drop),
            open @ (b'{' | b'[') => {
                let close = if open == b'{' { b'}' } else { b']' };
                self.at += 1;
                self.ws();
                if self.peek() == Some(close) {
                    self.at += 1;
                    return Ok(());
                }
                loop {
                    self.ws();
                    if open == b'{' {
                        self.string()?;
                        self.ws();
                        self.eat(b':')?;
                    }
                    self.skip_value(depth + 1)?;
                    self.ws();
                    match self.next() {
                        Some(b',') => continue,
                        Some(c) if c == close => return Ok(()),
                        other => return Err(format!("bad list at {}: {:?}", self.at, other.map(char::from))),
                    }
                }
            }
            _ => {
                let start = self.at;
                while self.peek().is_some_and(|c| !matches!(c, b',' | b'}' | b']') && !c.is_ascii_whitespace()) {
                    self.at += 1;
                }
                if self.at == start { Err(format!("bad value at {start}")) } else { Ok(()) }
            }
        }
    }
}

/// Which side of the game a map is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapMode {
    Multiplayer,
    Zombies,
}

/// Where a map was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapSource {
    /// A Steam Workshop item.
    Workshop,
    /// BO3's own `usermaps/<map>/zone/`.
    Usermaps,
}

/// A community map for Black Ops III.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bo3CustomMap {
    /// Its zone's name, lower case (`mp_warhawk`).
    pub stem: String,
    /// Its title (`workshop.json`'s, colour codes taken out; else from
    /// the zone name).
    pub title: String,
    /// The Workshop item's id (its folder's name), or the usermap's name.
    pub id: String,
    pub mode: MapMode,
    pub source: MapSource,
    /// The folder its zone is in (the item's folder).
    pub dir: PathBuf,
    /// Its zone (`<FolderName>.ff`).
    pub zone: PathBuf,
    /// Its streamed data (`<FolderName>.xpak` beside the zone), if any.
    pub xpak: Option<PathBuf>,
    /// Its pictures, best first (loading image, preview, Workshop picture).
    pub pictures: Vec<PathBuf>,
}

/// bo2mp: a Black Ops III Workshop mod (`Type` `mod`): a weapon pack's
/// zones sit flat in its folder, `core_mod.ff`, `zm_mod.ff` and `mp_mod.ff`
/// (whichever it has), each with its streamed data (`<zone>.xpak`) beside
/// it; the language copies (`en_core_mod.ff`, ...) are left out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bo3WorkshopMod {
    /// The Workshop item's id (its folder's name).
    pub id: String,
    /// Its title, colour codes taken out.
    pub title: String,
    /// `workshop.json`'s `FolderName`.
    pub folder_name: String,
    pub dir: PathBuf,
    /// Its zones, in `MOD_ZONES` order.
    pub zones: Vec<PathBuf>,
    /// The streamed data of those zones that have it.
    pub xpaks: Vec<PathBuf>,
}

/// The zones a mod may have, in the order they are listed.
pub const MOD_ZONES: [&str; 3] = ["core_mod", "zm_mod", "mp_mod"];

impl Bo3WorkshopMod {
    /// Its zone named `stem` (`zm_mod`), if it has one.
    pub fn zone(&self, stem: &str) -> Option<&Path> {
        self.zones.iter().find(|z| z.file_stem().is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case(stem))).map(PathBuf::as_path)
    }
}

/// What a search found.
#[derive(Clone, Debug, Default)]
pub struct Discovery {
    /// The Workshop content folders searched (`.../content/311210`).
    pub content_dirs: Vec<PathBuf>,
    pub maps: Vec<Bo3CustomMap>,
    /// bo2mp: the mods with at least one zone.
    pub mods: Vec<Bo3WorkshopMod>,
    /// Items that are not maps: (folder, title, type).
    pub not_maps: Vec<(PathBuf, String, String)>,
    /// Folders left out, and why.
    pub skipped: Vec<(PathBuf, String)>,
}

impl Discovery {
    pub fn map(&self, stem: &str) -> Option<&Bo3CustomMap> {
        self.maps.iter().find(|m| m.stem.eq_ignore_ascii_case(stem))
    }

    /// bo2mp: the mod with this Workshop id or `FolderName`.
    pub fn workshop_mod(&self, key: &str) -> Option<&Bo3WorkshopMod> {
        self.mods.iter().find(|m| m.id == key).or_else(|| self.mods.iter().find(|m| !m.folder_name.is_empty() && m.folder_name.eq_ignore_ascii_case(key)))
    }
}

/// The Steam library a game in `<library>/steamapps/common/<game>` is in.
pub fn library_of_game(game: &Path) -> Option<PathBuf> {
    let common = game.parent()?;
    let steamapps = common.parent()?;
    let is = |p: &Path, name: &str| p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name));
    (is(common, "common") && is(steamapps, "steamapps")).then(|| steamapps.parent().map(Path::to_path_buf)).flatten()
}

/// BO3's Workshop content folders: the one in BO3's own library first,
/// then every other library's, those that exist, each once.
pub fn workshop_content_dirs(bo3: Option<&Path>, libraries: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    for lib in bo3.and_then(library_of_game).into_iter().chain(libraries.iter().cloned()) {
        let dir = lib.join("steamapps").join("workshop").join("content").join(BO3_APP_ID);
        let key = dir.to_string_lossy().to_ascii_lowercase().replace('\\', "/");
        if !keys.contains(&key) && dir.is_dir() {
            keys.push(key);
            out.push(dir);
        }
    }
    out
}

/// A file in `dir` named `name` (any case).
fn file_in(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.is_file() {
        return Some(exact);
    }
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.is_file() && p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name)))
}

/// The pictures of an item (in `dirs`), best first: the loading image, the
/// preview, the Workshop picture, then any other picture (backups and
/// Photoshop files left out).
fn pictures_in(dirs: &[&Path]) -> Vec<PathBuf> {
    const BEST: [&str; 6] = ["loadingimage.png", "previewimage.png", "workshop.jpg", "workshop.png", "thumbnail.png", "thumbnail.jpg"];
    let mut out: Vec<PathBuf> = Vec::new();
    for name in BEST {
        out.extend(dirs.iter().filter_map(|d| file_in(d, name)));
    }
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        let mut rest: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                let name = p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
                (name.ends_with(".png") || name.ends_with(".jpg") || name.ends_with(".jpeg")) && !name.contains("backup") && !name.contains("_bk")
            })
            .filter(|p| !out.contains(p))
            .collect();
        rest.sort();
        out.extend(rest);
    }
    // (The mod tools' stand-in pictures are a few KB: a real one first.)
    let tiny = |p: &PathBuf| std::fs::metadata(p).is_ok_and(|m| m.len() < 16 * 1024);
    let (small, mut big): (Vec<PathBuf>, Vec<PathBuf>) = out.into_iter().partition(tiny);
    big.extend(small);
    big
}

/// A title without the game's colour codes (`^1`).
fn clean_title(title: &str) -> String {
    let mut out = String::new();
    let mut chars = title.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '^' && chars.peek().is_some_and(char::is_ascii_digit) {
            chars.next();
            continue;
        }
        out.push(c);
    }
    // (Spaces inside brackets dropped: `Dome ( Nuketown reimagined )` ->
    // `Dome (Nuketown reimagined)`.)
    out.split_whitespace().collect::<Vec<_>>().join(" ").replace("( ", "(").replace(" )", ")")
}

/// bo2mp: a title for the menus: one written all in capitals
/// (`FIRING RANGE`) in the games' own way (`Firing Range`); others as they
/// are.
pub fn display_title(title: &str) -> String {
    if title.chars().any(char::is_lowercase) || !title.chars().any(char::is_uppercase) {
        return title.to_owned();
    }
    title
        .split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A title from a zone name (`mp_killhouse_z` -> `Killhouse Z`).
fn title_from_stem(stem: &str) -> String {
    stem.trim_start_matches("mp_")
        .trim_start_matches("zm_")
        .split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Multiplayer or Zombies, by its zone's name, else its tags.
fn mode_of(stem: &str, tags: &str) -> Option<MapMode> {
    let tags = tags.to_ascii_lowercase();
    if stem.starts_with("mp_") {
        Some(MapMode::Multiplayer)
    } else if stem.starts_with("zm_") {
        Some(MapMode::Zombies)
    } else if tags.contains("multiplayer") {
        Some(MapMode::Multiplayer)
    } else if tags.contains("zombies") {
        Some(MapMode::Zombies)
    } else {
        None
    }
}

/// Reads one map folder: `item` (the folder named for it), `zone_dir`
/// (where its zone is), `json` (its `workshop.json`, if any).
fn read_item(
    out: &mut Discovery,
    item: &Path,
    zone_dir: &Path,
    json: Option<WorkshopJson>,
    fallback_name: &str,
    source: MapSource,
) {
    let json = json.unwrap_or_default();
    if !json.kind.is_empty() && !json.kind.eq_ignore_ascii_case("map") {
        let title = if json.title.is_empty() { json.folder_name.clone() } else { clean_title(&json.title) };
        if json.kind.eq_ignore_ascii_case("mod") && source == MapSource::Workshop {
            let zones: Vec<PathBuf> = MOD_ZONES.iter().filter_map(|z| file_in(zone_dir, &format!("{z}.ff"))).collect();
            if !zones.is_empty() {
                out.mods.push(Bo3WorkshopMod {
                    id: item.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    title: title.clone(),
                    folder_name: json.folder_name.trim().to_owned(),
                    dir: zone_dir.to_path_buf(),
                    xpaks: MOD_ZONES.iter().filter_map(|z| file_in(zone_dir, &format!("{z}.xpak"))).collect(),
                    zones,
                });
            }
        }
        out.not_maps.push((item.to_path_buf(), title, json.kind));
        return;
    }
    let folder_name = if json.folder_name.trim().is_empty() { fallback_name.to_owned() } else { json.folder_name.trim().to_owned() };
    if folder_name.is_empty() {
        out.skipped.push((item.to_path_buf(), "workshop.json names no FolderName".into()));
        return;
    }
    let Some(zone) = file_in(zone_dir, &format!("{folder_name}.ff")) else {
        out.skipped.push((item.to_path_buf(), format!("no {folder_name}.ff in {}", zone_dir.display())));
        return;
    };
    let stem = folder_name.to_ascii_lowercase();
    let Some(mode) = mode_of(&stem, &json.tags) else {
        out.skipped.push((item.to_path_buf(), format!("{folder_name}: neither a multiplayer (mp_) nor a Zombies (zm_) map")));
        return;
    };
    if let Some(first) = out.map(&stem) {
        out.skipped.push((item.to_path_buf(), format!("{stem} already found in {}", first.dir.display())));
        return;
    }
    let title = clean_title(&json.title);
    let dirs: Vec<&Path> = if zone_dir == item { vec![item] } else { vec![zone_dir, item] };
    out.maps.push(Bo3CustomMap {
        title: if title.is_empty() { title_from_stem(&stem) } else { title },
        id: item.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        mode,
        source,
        dir: zone_dir.to_path_buf(),
        xpak: file_in(zone_dir, &format!("{folder_name}.xpak")),
        pictures: pictures_in(&dirs),
        zone,
        stem,
    });
}

fn read_json(dir: &Path) -> Option<Result<WorkshopJson, String>> {
    let path = file_in(dir, "workshop.json")?;
    Some(std::fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| parse_workshop_json(&t)))
}

fn sorted_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    dirs
}

/// Searches Workshop content folders (`.../content/311210`) and BO3
/// `usermaps` folders for maps.
pub fn discover_in(content_dirs: &[PathBuf], usermaps_dirs: &[PathBuf]) -> Discovery {
    let mut out = Discovery { content_dirs: content_dirs.to_vec(), ..Default::default() };
    for content in content_dirs {
        for item in sorted_dirs(content) {
            match read_json(&item) {
                None => out.skipped.push((item.clone(), "no workshop.json".into())),
                Some(Err(e)) => out.skipped.push((item.clone(), format!("workshop.json unreadable: {e}"))),
                Some(Ok(json)) => read_item(&mut out, &item, &item, Some(json), "", MapSource::Workshop),
            }
        }
    }
    for usermaps in usermaps_dirs {
        for item in sorted_dirs(usermaps) {
            let name = item.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let zone_dir = Some(item.join("zone")).filter(|z| z.is_dir()).unwrap_or_else(|| item.clone());
            let json = match read_json(&zone_dir).or_else(|| read_json(&item)) {
                Some(Ok(j)) => Some(j),
                Some(Err(e)) => {
                    out.skipped.push((item.clone(), format!("workshop.json unreadable: {e}")));
                    None
                }
                None => None,
            };
            read_item(&mut out, &item, &zone_dir, json, &name, MapSource::Usermaps);
        }
    }
    out
}

/// BO3's folder, as set (`game_paths.cfg` `bo3=`).
fn bo3_folder() -> Option<PathBuf> {
    crate::game_paths::get("bo3").filter(|p| p.is_dir())
}

/// BO3's `zone` folder (its shared zones and packages), when its folder is
/// set.
pub fn bo3_zone_dir() -> Option<PathBuf> {
    bo3_folder().map(|f| f.join("zone")).filter(|z| z.is_dir())
}

/// His Black Ops III community maps, found once a run (and logged once).
pub fn bo3() -> &'static Discovery {
    static FOUND: OnceLock<Discovery> = OnceLock::new();
    FOUND.get_or_init(|| {
        let setting = codlegacy_env::var("CODLEGACY_BO3_WORKSHOP").unwrap_or_default();
        if setting.trim() == "0" {
            diag::info!(Zone, "bo3 workshop: off (CODLEGACY_BO3_WORKSHOP=0)");
            return Discovery::default();
        }
        let bo3 = bo3_folder().or_else(|| {
            crate::game_scan::wait_if_started();
            bo3_folder()
        });
        let mut content = workshop_content_dirs(bo3.as_deref(), &crate::game_scan::steam_libraries());
        content.extend(setting.split(';').map(str::trim).filter(|s| !s.is_empty()).map(PathBuf::from).filter(|p| p.is_dir()));
        let usermaps: Vec<PathBuf> = bo3.iter().map(|b| b.join("usermaps")).filter(|u| u.is_dir()).collect();
        let found = discover_in(&content, &usermaps);
        log(&found, bo3.as_deref(), &usermaps);
        found
    })
}

fn log(found: &Discovery, bo3: Option<&Path>, usermaps: &[PathBuf]) {
    diag::info!(
        Zone,
        "bo3 workshop: found {} maps ({}), {} not maps",
        found.maps.len(),
        found.maps.iter().map(|m| format!("{} '{}'", m.stem, m.title)).collect::<Vec<_>>().join(", "),
        found.not_maps.len()
    );
    diag::info!(
        Zone,
        "bo3 workshop: searched {} (BO3 at {}); usermaps: {}",
        if found.content_dirs.is_empty() { "no Workshop folder".to_owned() } else { found.content_dirs.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(", ") },
        bo3.map_or_else(|| "not set".to_owned(), |b| b.display().to_string()),
        if usermaps.is_empty() { "none".to_owned() } else { usermaps.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(", ") }
    );
    for m in &found.maps {
        diag::info!(
            Zone,
            "bo3 workshop: {} '{}' ({}, {}) from {} (zone {}, streamed data {}, picture {})",
            m.stem,
            m.title,
            if m.mode == MapMode::Multiplayer { "multiplayer" } else { "zombies: not listed, the Zombies side plays only BO3's own maps" },
            if m.source == MapSource::Workshop { format!("Workshop item {}", m.id) } else { "usermaps".to_owned() },
            m.dir.display(),
            m.zone.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            m.xpak.as_ref().and_then(|x| x.file_name()).map_or_else(|| "none".to_owned(), |n| n.to_string_lossy().into_owned()),
            m.pictures.first().and_then(|p| p.file_name()).map_or_else(|| "none".to_owned(), |n| n.to_string_lossy().into_owned()),
        );
    }
    for (dir, title, kind) in &found.not_maps {
        diag::info!(Zone, "bo3 workshop: not a map ({kind}): '{title}' in {}", dir.display());
    }
    for m in &found.mods {
        let names = |v: &[PathBuf]| v.iter().filter_map(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).collect::<Vec<_>>().join(", ");
        diag::info!(Zone, "bo3 workshop: mod {} '{}' ({}): zones {}; streamed data {}", m.id, m.title, m.folder_name, names(&m.zones), names(&m.xpaks));
    }
    for (dir, why) in &found.skipped {
        diag::info!(Zone, "bo3 workshop: left out {}: {why}", dir.display());
    }
}

/// His BO3 community map with this zone name.
pub fn bo3_map(stem: &str) -> Option<&'static Bo3CustomMap> {
    bo3().map(stem)
}

/// bo2mp: his BO3 Workshop mod with this item id or `FolderName`.
pub fn bo3_mod(key: &str) -> Option<&'static Bo3WorkshopMod> {
    bo3().workshop_mod(key)
}

/// The folders of his Workshop maps (multiplayer and Zombies: the menu
/// lists both), searched for zones with the games' (BO3's `usermaps` are
/// inside BO3's folder, searched already).
pub(crate) fn zone_roots() -> Vec<PathBuf> {
    bo3()
        .maps
        .iter()
        .filter(|m| m.source == MapSource::Workshop)
        .map(|m| m.dir.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bo2mp-workshop-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn json(folder: &str, title: &str, kind: &str) -> String {
        format!(
            "{{\n    \"Description\": \"A map.\\r\\nWith \\\"quotes\\\" and \\u00e9.\",\n    \"FolderName\": \"{folder}\",\n    \"PublisherID\": \"1\",\n    \"Tags\": \"Map,Multiplayer\",\n    \"Thumbnail\": \"E:/x/workshop.jpg\",\n    \"Title\": \"{title}\",\n    \"Type\": \"{kind}\"\n}}\n"
        )
    }

    #[test]
    fn workshop_json_is_read() {
        let j = parse_workshop_json(&json("mp_warhawk", "Warhawk", "map")).unwrap();
        assert_eq!(j.folder_name, "mp_warhawk");
        assert_eq!(j.title, "Warhawk");
        assert_eq!(j.kind, "map");
        assert_eq!(j.tags, "Map,Multiplayer");
        assert_eq!(j.publisher_id, "1");
        assert_eq!(j.description, "A map.\r\nWith \"quotes\" and \u{e9}.");
        // Other kinds of values are skipped; a BOM and odd key case are fine.
        let j = parse_workshop_json("\u{feff}{\"n\": 12.5, \"a\": [1, {\"x\": \"y\"}], \"b\": true, \"type\": \"mod\", \"o\": {}}").unwrap();
        assert_eq!(j.kind, "mod");
        assert_eq!(parse_workshop_json("{}").unwrap(), WorkshopJson::default());
        assert!(parse_workshop_json("{\"Title\": \"x\"").is_err());
        assert!(parse_workshop_json("not json").is_err());
        // The real thing (his Warhawk, as Steam wrote it).
        let real = "{\n    \"Description\": \"Warhawk sees the action brought to small town USA with a fan favorite from Call of Duty: Ghosts re-imagined in Call of Duty: Black Ops 3.\",\n    \"FolderName\": \"mp_warhawk\",\n    \"PublisherID\": \"1705100612\",\n    \"Tags\": \"Map,Multiplayer\",\n    \"Thumbnail\": \"E:/SteamLibrary/steamapps/common/Call of Duty Black Ops III/usermaps/mp_warhawk/zone/workshop.jpg\",\n    \"Title\": \"Warhawk\",\n    \"Type\": \"map\"\n}";
        let j = parse_workshop_json(real).unwrap();
        assert_eq!((j.title.as_str(), j.folder_name.as_str(), j.publisher_id.as_str()), ("Warhawk", "mp_warhawk", "1705100612"));
    }

    #[test]
    fn titles_are_cleaned() {
        assert_eq!(clean_title("^1Nuketown ^768"), "Nuketown 68");
        assert_eq!(clean_title("  FIRING   RANGE "), "FIRING RANGE");
        assert_eq!(title_from_stem("mp_killhouse_z"), "Killhouse Z");
        assert_eq!(clean_title("Dome ( Nuketown reimagined )"), "Dome (Nuketown reimagined)");
        assert_eq!(clean_title("Mob Of The Dead Remastered ( Beta )"), "Mob Of The Dead Remastered (Beta)");
        assert_eq!(display_title("FIRING RANGE"), "Firing Range");
        assert_eq!(display_title("SUMMIT"), "Summit");
        assert_eq!(display_title("Shoot House 1918"), "Shoot House 1918");
        assert_eq!(display_title("the giant"), "the giant");
    }

    #[test]
    fn items_are_found_and_others_left_out() {
        let t = tmp("discover");
        let content = t.join("lib").join("steamapps").join("workshop").join("content").join(BO3_APP_ID);
        // A multiplayer map with its streamed data and pictures.
        let a = content.join("1705100612");
        write(&a.join("workshop.json"), &json("mp_warhawk", "Warhawk", "map"));
        write(&a.join("mp_warhawk.ff"), "");
        write(&a.join("en_mp_warhawk.ff"), "");
        write(&a.join("mp_warhawk.xpak"), "");
        write(&a.join("previewimage.png"), "");
        write(&a.join("loadingimage.png"), "");
        write(&a.join("loadingimage_backup.png"), "");
        // A Zombies map (zone named in another case).
        let z = content.join("1680193042");
        write(&z.join("workshop.json"), &json("zm_town_hd", "Town", "map"));
        write(&z.join("ZM_TOWN_HD.ff"), "");
        // A mod: not a map.
        let m = content.join("2079910029");
        write(&m.join("workshop.json"), &json("brad_pm_menu", "Menu", "mod"));
        write(&m.join("mp_mod.ff"), "");
        write(&m.join("en_mp_mod.ff"), "");
        // A map without its zone; a folder without workshop.json.
        let n = content.join("3");
        write(&n.join("workshop.json"), &json("mp_gone", "Gone", "map"));
        write(&content.join("4").join("x.ff"), "");
        // A usermap in BO3's own folder (no workshop.json: named by its folder).
        let usermaps = t.join("bo3").join("usermaps");
        write(&usermaps.join("mp_mine").join("zone").join("mp_mine.ff"), "");
        write(&usermaps.join("mp_mine").join("zone").join("workshop.jpg"), "");

        let found = discover_in(std::slice::from_ref(&content), std::slice::from_ref(&usermaps));
        let stems: Vec<&str> = found.maps.iter().map(|m| m.stem.as_str()).collect();
        assert_eq!(stems, ["zm_town_hd", "mp_warhawk", "mp_mine"]);
        let w = found.map("mp_warhawk").unwrap();
        assert_eq!(w.title, "Warhawk");
        assert_eq!(w.id, "1705100612");
        assert_eq!((w.mode, w.source), (MapMode::Multiplayer, MapSource::Workshop));
        assert_eq!(w.zone, a.join("mp_warhawk.ff"));
        assert_eq!(w.xpak.as_deref(), Some(a.join("mp_warhawk.xpak").as_path()));
        assert_eq!(w.pictures, [a.join("loadingimage.png"), a.join("previewimage.png")]);
        // A stand-in loading image (a few KB) goes after a real preview.
        write(&z.join("loadingimage.png"), "x");
        write(&z.join("previewimage.png"), &"x".repeat(20_000));
        assert_eq!(pictures_in(&[z.as_path()]), [z.join("previewimage.png"), z.join("loadingimage.png")]);
        let town = found.map("zm_town_hd").unwrap();
        assert_eq!(town.mode, MapMode::Zombies);
        assert!(town.xpak.is_none());
        let mine = found.map("mp_mine").unwrap();
        assert_eq!((mine.title.as_str(), mine.source), ("Mine", MapSource::Usermaps));
        assert_eq!(mine.dir, usermaps.join("mp_mine").join("zone"));
        assert_eq!(mine.pictures, [usermaps.join("mp_mine").join("zone").join("workshop.jpg")]);
        assert_eq!(found.not_maps.len(), 1);
        assert_eq!(found.not_maps[0].2, "mod");
        // The mod is kept with its zones (a weapon pack's).
        let wm = found.workshop_mod("2079910029").unwrap();
        assert_eq!(wm.zones, [m.join("mp_mod.ff")]);
        assert!(wm.xpaks.is_empty());
        assert_eq!(found.workshop_mod("BRAD_PM_MENU").map(|w| w.id.as_str()), Some("2079910029"));
        assert_eq!(wm.zone("mp_mod"), Some(m.join("mp_mod.ff").as_path()));
        assert!(wm.zone("zm_mod").is_none());
        assert_eq!(found.skipped.len(), 2, "{:?}", found.skipped);
        assert!(found.skipped.iter().any(|(_, why)| why.contains("no mp_gone.ff")));
        assert!(found.skipped.iter().any(|(_, why)| why == "no workshop.json"));
        let _ = std::fs::remove_dir_all(&t);
    }

    #[test]
    fn libraries_are_resolved() {
        let t = tmp("libs");
        let lib = t.join("Steam");
        let bo3 = lib.join("steamapps").join("common").join("Call of Duty Black Ops III");
        std::fs::create_dir_all(&bo3).unwrap();
        assert_eq!(library_of_game(&bo3), Some(lib.clone()));
        assert_eq!(library_of_game(&t.join("Games").join("BO3")), None);
        let content = lib.join("steamapps").join("workshop").join("content").join(BO3_APP_ID);
        std::fs::create_dir_all(&content).unwrap();
        let other = t.join("D").join("SteamLibrary");
        let other_content = other.join("steamapps").join("workshop").join("content").join(BO3_APP_ID);
        std::fs::create_dir_all(&other_content).unwrap();
        let empty = t.join("E").join("SteamLibrary");
        std::fs::create_dir_all(&empty).unwrap();
        // BO3's own library first; each once; libraries without the folder left out.
        let dirs = workshop_content_dirs(Some(&bo3), &[other.clone(), lib.clone(), empty]);
        assert_eq!(dirs, [content.clone(), other_content.clone()]);
        // Without BO3's folder set, the libraries alone.
        assert_eq!(workshop_content_dirs(None, &[lib]), [content]);
        // The libraries Steam lists (`libraryfolders.vdf`).
        let vdf = format!("\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n", other.display().to_string().replace('\\', "\\\\"));
        let libs = crate::game_scan::vdf_libraries(&vdf);
        assert_eq!(workshop_content_dirs(None, &libs), [other_content]);
        let _ = std::fs::remove_dir_all(&t);
    }
}
