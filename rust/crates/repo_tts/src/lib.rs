//! R.E.P.O.'s text-to-speech voice for the chat.
//!
//! REPO speaks typed chat with Klattersynth (Strobotnik's Klatt formant
//! synthesiser, `REPO_Data/Managed/Klattersynth.dll`), a .NET assembly run by
//! Unity's Mono. Nothing of it is copied here: the owner's own install is
//! used as it is.
//!
//! - REPO's Mono runtime (`MonoBleedingEdge/EmbedRuntime/mono-2.0-bdwgc.dll`)
//!   is loaded at runtime ([`mono`], the crate's only `unsafe`), with REPO's
//!   own class libraries (`REPO_Data/Managed`).
//! - Klattersynth calls a few UnityEngine members (maths, `AudioClip.Create`,
//!   an `AudioSource`). Outside Unity those would crash, so a stand-in
//!   `UnityEngine` assembly (`dotnet/UnityEngine.cs`, ours) is found first:
//!   `AudioClip.Create` keeps the clip's sample reader, which is then read.
//! - A small driver (`dotnet/Bo2Tts.cs`, ours) makes a line the way REPO's
//!   `TTSVoice` does: word by word, each word one clip, with the settings of
//!   REPO's player voice ([`Voice::DEFAULT`], read from its prefab).
//!
//! All of it runs on one thread of its own ([`Tts`]); a line comes back as
//! mono 11025 Hz samples. No install, or anything failing, is logged once
//! and the chat stays silent.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

mod mono;

pub use mono::Setup;

/// The stand-in `UnityEngine` and the driver, written beside the game when
/// first needed (`dotnet/build.sh` rebuilds them).
const UNITY_STAND_IN: &[u8] = include_bytes!("../dotnet/UnityEngine.dll");
const DRIVER: &[u8] = include_bytes!("../dotnet/Bo2Tts.dll");

/// The sample rate Klattersynth makes REPO's voice at.
pub const SAMPLE_RATE: u32 = 11025;

/// A Klattersynth voice: REPO's player prefab (`TTS` → `Voice Default`,
/// `Voice Whisper`, their `Speech` components) sets these.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Voice {
    pub base_frequency: i32,
    pub whisper: bool,
    pub ms_per_speech_frame: i32,
    pub flutter: i32,
    pub flutter_speed: f32,
}

impl Voice {
    /// REPO's voice: 220 Hz, natural voicing, 10 ms frames, flutter 10.
    pub const DEFAULT: Self = Self {
        base_frequency: 220,
        whisper: false,
        ms_per_speech_frame: 10,
        flutter: 10,
        flutter_speed: 1.0,
    };
    /// REPO's crouched voice: a 400 Hz whisper, 20 ms frames.
    pub const WHISPER: Self = Self {
        base_frequency: 400,
        whisper: true,
        ms_per_speech_frame: 20,
        flutter: 10,
        flutter_speed: 1.0,
    };
}

/// Where REPO is installed: `BO2MP_REPO_DIR`, else Steam's usual folders
/// and the Steam libraries `libraryfolders.vdf` lists. `None` without one
/// holding Klattersynth.
#[must_use]
pub fn find_install() -> Option<PathBuf> {
    let mut tried: Vec<PathBuf> = Vec::new();
    if let Some(dir) = std::env::var_os("BO2MP_REPO_DIR") {
        tried.push(PathBuf::from(dir));
    }
    let steams = [
        r"C:\Program Files (x86)\Steam",
        r"C:\Program Files\Steam",
    ];
    for steam in steams {
        let steam = Path::new(steam);
        tried.push(steam.join("steamapps").join("common").join("REPO"));
        for lib in steam_libraries(&steam.join("steamapps").join("libraryfolders.vdf")) {
            tried.push(lib.join("steamapps").join("common").join("REPO"));
        }
    }
    tried.into_iter().find(|d| Install::at(d).is_some())
}

/// The `"path"` entries of Steam's `libraryfolders.vdf`.
fn steam_libraries(vdf: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(vdf) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix("\"path\"")?.trim();
            let v = rest.strip_prefix('"')?.strip_suffix('"')?;
            Some(PathBuf::from(v.replace("\\\\", "\\")))
        })
        .collect()
}

/// The parts of a REPO install used.
#[derive(Clone, Debug)]
pub struct Install {
    /// `REPO_Data/Managed`: Klattersynth and REPO's class libraries.
    pub managed: PathBuf,
    /// The Mono runtime library.
    pub runtime: PathBuf,
    /// Mono's `etc` folder.
    pub config: PathBuf,
}

impl Install {
    /// The install at `root`, if it has what is needed.
    #[must_use]
    pub fn at(root: &Path) -> Option<Self> {
        let managed = root.join("REPO_Data").join("Managed");
        let mono = root.join("MonoBleedingEdge");
        let runtime = mono.join("EmbedRuntime").join("mono-2.0-bdwgc.dll");
        (managed.join("Klattersynth.dll").is_file() && runtime.is_file()).then(|| Self {
            managed,
            runtime,
            config: mono.join("etc"),
        })
    }
}

/// Where the stand-in and driver are written (beside the game's other
/// caches).
fn stand_in_dir() -> PathBuf {
    Path::new("codlegacy-artifacts").join("cache").join("repo_tts")
}

struct Request {
    text: String,
    voice: Voice,
    reply: mpsc::Sender<Result<Vec<f32>, String>>,
}

/// REPO's voice on a thread of its own. Cheap to clone.
#[derive(Clone)]
pub struct Tts {
    tx: mpsc::Sender<Request>,
}

impl Tts {
    /// Starts the voice thread with REPO's runtime and Klattersynth from
    /// `install`. The runtime loads when the first line is asked for.
    #[must_use]
    pub fn start(install: Install) -> Self {
        Self::start_with(mono::Setup::repo(&install, stand_in_dir()))
    }

    /// Starts it with a given runtime setup (tests use a system Mono).
    #[must_use]
    pub fn start_with(setup: mono::Setup) -> Self {
        let (tx, rx) = mpsc::channel::<Request>();
        let spawned = std::thread::Builder::new().name("repo-tts".into()).spawn(move || {
            let mut host: Option<Result<mono::Host, String>> = None;
            for req in rx {
                let host = host.get_or_insert_with(|| {
                    let made = mono::Host::new(&setup);
                    match &made {
                        Ok(_) => diag::info!(Audio, "chat voice: REPO's Klattersynth is ready ({})", setup.runtime.display()),
                        Err(e) => diag::warn!(Audio, "chat voice: REPO's Klattersynth did not start: {e}"),
                    }
                    made
                });
                let out = match host {
                    Ok(h) => h.speak(&ascii_line(&req.text), req.voice),
                    Err(e) => Err(e.clone()),
                };
                let _ = req.reply.send(out);
            }
        });
        if let Err(e) = spawned {
            diag::warn!(Audio, "chat voice: no thread: {e}");
        }
        Self { tx }
    }

    /// Asks for a line; the samples (mono, [`SAMPLE_RATE`]) come on the
    /// receiver when made.
    pub fn speak(&self, text: &str, voice: Voice) -> mpsc::Receiver<Result<Vec<f32>, String>> {
        let (reply, rx) = mpsc::channel();
        let _ = self.tx.send(Request {
            text: text.to_owned(),
            voice,
            reply,
        });
        rx
    }
}

/// A chat line as Klattersynth takes it: 7-bit ASCII only (it refuses
/// anything else), accented letters as their plain ones (REPO's
/// `TranslateSpecialLetters` does likewise), colour codes (`^1`) dropped,
/// at most 200 characters.
#[must_use]
pub fn ascii_line(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '^' && chars.peek().is_some_and(char::is_ascii_digit) {
            chars.next();
            continue;
        }
        let plain = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => "a",
            'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => "A",
            'è' | 'é' | 'ê' | 'ë' => "e",
            'È' | 'É' | 'Ê' | 'Ë' => "E",
            'ì' | 'í' | 'î' | 'ï' => "i",
            'Ì' | 'Í' | 'Î' | 'Ï' => "I",
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => "o",
            'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' => "O",
            'ù' | 'ú' | 'û' | 'ü' => "u",
            'Ù' | 'Ú' | 'Û' | 'Ü' => "U",
            'ñ' => "n",
            'Ñ' => "N",
            'ç' => "c",
            'Ç' => "C",
            'ß' => "ss",
            'æ' => "ae",
            'Æ' => "AE",
            '’' | '‘' => "'",
            c if c.is_ascii() && !c.is_ascii_control() => {
                out.push(c);
                continue;
            }
            c if c.is_whitespace() => " ",
            _ => "",
        };
        out.push_str(plain);
    }
    out.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_plain_ascii() {
        assert_eq!(ascii_line("^1héllo  wörld\tok"), "hello  world ok");
        assert_eq!(ascii_line("gg 🙂 ez"), "gg  ez");
        assert_eq!(ascii_line(&"a".repeat(500)).len(), 200);
    }

    #[test]
    fn steam_library_paths_are_read() {
        let dir = std::env::temp_dir().join("repo_tts_vdf_test");
        let _ = std::fs::create_dir_all(&dir);
        let vdf = dir.join("libraryfolders.vdf");
        std::fs::write(&vdf, "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t}\n}\n").unwrap();
        assert_eq!(steam_libraries(&vdf), vec![PathBuf::from("D:\\SteamLibrary")]);
    }

    /// REPO's Klattersynth run by a system Mono: `REPO_TTS_MONO=<libmonosgen
    /// -2.0.so> REPO_TTS_MONO_LIB=<mono lib dir> REPO_TTS_MONO_ETC=<mono etc>
    /// REPO_TTS_MANAGED=<REPO_Data/Managed> cargo test -p repo_tts --
    /// --ignored --nocapture`. Writes `REPO_TTS_WAV` when set.
    #[test]
    #[ignore = "needs a Mono runtime and REPO's Klattersynth.dll"]
    fn speaks_with_a_system_mono() {
        let var = |k: &str| PathBuf::from(std::env::var_os(k).unwrap_or_else(|| panic!("{k}")));
        let setup = mono::Setup {
            runtime: var("REPO_TTS_MONO"),
            assembly_root: var("REPO_TTS_MONO_LIB"),
            config: var("REPO_TTS_MONO_ETC"),
            managed: var("REPO_TTS_MANAGED"),
            stand_in_dir: std::env::temp_dir().join("repo_tts_stand_in"),
        };
        let tts = Tts::start_with(setup);
        for voice in [Voice::DEFAULT, Voice::WHISPER] {
            let samples = tts.speak("text chat example", voice).recv().unwrap().unwrap();
            let secs = samples.len() as f32 / SAMPLE_RATE as f32;
            let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            println!("{voice:?}: {} samples ({secs:.2} s), peak {peak:.2}", samples.len());
            assert!(secs > 0.8 && secs < 4.0 && peak > 0.1 && peak <= 1.0);
            if let (Some(out), false) = (std::env::var_os("REPO_TTS_WAV"), voice.whisper) {
                write_wav(Path::new(&out), &samples);
            }
        }
    }

    fn write_wav(path: &Path, s: &[f32]) {
        let mut b = Vec::new();
        let n = s.len() as u32 * 2;
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + n).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
        b.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&n.to_le_bytes());
        for v in s {
            b.extend_from_slice(&((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
        std::fs::write(path, b).unwrap();
    }
}
