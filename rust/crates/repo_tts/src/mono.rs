//! A Mono runtime loaded at runtime (REPO's own, `mono-2.0-bdwgc.dll`, or a
//! system one in tests) through its embedding API: the crate's only module
//! with `unsafe` (the symbol lookups and the foreign calls). Used from the
//! voice thread only; a process runs one Mono domain, so the host is made
//! once and kept.
#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::{Path, PathBuf};

use crate::Voice;

/// What the runtime is started with.
#[derive(Clone, Debug)]
pub struct Setup {
    /// The runtime library.
    pub runtime: PathBuf,
    /// Its class-library root (`mono_set_dirs`' assembly folder).
    pub assembly_root: PathBuf,
    /// Its `etc` folder.
    pub config: PathBuf,
    /// The folder with Klattersynth.dll (REPO's `REPO_Data/Managed`, also
    /// its class libraries).
    pub managed: PathBuf,
    /// Where the stand-in `UnityEngine.dll` and the driver are written; it
    /// is searched before `managed`, so the stand-in is the `UnityEngine`
    /// Klattersynth binds to.
    pub stand_in_dir: PathBuf,
}

impl Setup {
    /// REPO's runtime and libraries, as Unity starts them.
    pub(crate) fn repo(install: &crate::Install, stand_in_dir: PathBuf) -> Self {
        Self {
            runtime: install.runtime.clone(),
            assembly_root: install.managed.clone(),
            config: install.config.clone(),
            managed: install.managed.clone(),
            stand_in_dir,
        }
    }
}

type Ptr = *mut c_void;

struct Api {
    string_new: unsafe extern "C" fn(Ptr, *const c_char) -> Ptr,
    runtime_invoke: unsafe extern "C" fn(Ptr, Ptr, *mut Ptr, *mut Ptr) -> Ptr,
    array_length: unsafe extern "C" fn(Ptr) -> usize,
    array_addr_with_size: unsafe extern "C" fn(Ptr, c_int, usize) -> *mut c_char,
    object_to_string: unsafe extern "C" fn(Ptr, *mut Ptr) -> Ptr,
    string_to_utf8: unsafe extern "C" fn(Ptr) -> *mut c_char,
    free: unsafe extern "C" fn(Ptr),
}

/// The runtime with the driver loaded.
pub(crate) struct Host {
    api: Api,
    domain: Ptr,
    speak: Ptr,
    last_error: Ptr,
    // Keeps the runtime mapped while its functions may be called.
    _lib: libloading::Library,
}

// (The host lives on the voice thread only.)
unsafe impl Send for Host {}

fn cpath(p: &Path) -> Result<CString, String> {
    CString::new(p.to_string_lossy().into_owned()).map_err(|_| format!("path with a NUL: {}", p.display()))
}

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap_or_default()
}

impl Host {
    pub(crate) fn new(setup: &Setup) -> Result<Self, String> {
        std::fs::create_dir_all(&setup.stand_in_dir).map_err(|e| format!("{}: {e}", setup.stand_in_dir.display()))?;
        let unity = setup.stand_in_dir.join("UnityEngine.dll");
        let driver = setup.stand_in_dir.join("Bo2Tts.dll");
        for (path, bytes) in [(&unity, crate::UNITY_STAND_IN), (&driver, crate::DRIVER)] {
            if std::fs::read(path).ok().as_deref() != Some(bytes) {
                std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            }
        }
        let sep = if cfg!(windows) { ";" } else { ":" };
        let search = format!("{}{sep}{}", setup.stand_in_dir.display(), setup.managed.display());
        // SAFETY: the library is Mono's runtime; each symbol is looked up by
        // its embedding-API name and given that function's C signature.
        unsafe {
            let lib = libloading::Library::new(&setup.runtime).map_err(|e| format!("{}: {e}", setup.runtime.display()))?;
            macro_rules! sym {
                ($name:literal) => {
                    *lib.get(concat!($name, "\0").as_bytes()).map_err(|e| format!("{}: {e}", $name))?
                };
            }
            let set_dirs: unsafe extern "C" fn(*const c_char, *const c_char) = sym!("mono_set_dirs");
            let set_assemblies_path: unsafe extern "C" fn(*const c_char) = sym!("mono_set_assemblies_path");
            let jit_init: unsafe extern "C" fn(*const c_char) -> Ptr = sym!("mono_jit_init");
            let assembly_open: unsafe extern "C" fn(Ptr, *const c_char) -> Ptr = sym!("mono_domain_assembly_open");
            let get_image: unsafe extern "C" fn(Ptr) -> Ptr = sym!("mono_assembly_get_image");
            let class_from_name: unsafe extern "C" fn(Ptr, *const c_char, *const c_char) -> Ptr = sym!("mono_class_from_name");
            let method_from_name: unsafe extern "C" fn(Ptr, *const c_char, c_int) -> Ptr = sym!("mono_class_get_method_from_name");
            let api = Api {
                string_new: sym!("mono_string_new"),
                runtime_invoke: sym!("mono_runtime_invoke"),
                array_length: sym!("mono_array_length"),
                array_addr_with_size: sym!("mono_array_addr_with_size"),
                object_to_string: sym!("mono_object_to_string"),
                string_to_utf8: sym!("mono_string_to_utf8"),
                free: sym!("mono_free"),
            };
            let (root, config, search) = (cpath(&setup.assembly_root)?, cpath(&setup.config)?, cstr(&search));
            set_dirs(root.as_ptr(), config.as_ptr());
            set_assemblies_path(search.as_ptr());
            let domain = jit_init(cstr("bo2mp-repo-tts").as_ptr());
            if domain.is_null() {
                return Err("mono_jit_init failed".into());
            }
            // (The stand-in first, so it is the UnityEngine bound to.)
            if assembly_open(domain, cpath(&unity)?.as_ptr()).is_null() {
                return Err(format!("{} did not load", unity.display()));
            }
            let asm = assembly_open(domain, cpath(&driver)?.as_ptr());
            if asm.is_null() {
                return Err(format!("{} did not load", driver.display()));
            }
            let class = class_from_name(get_image(asm), cstr("").as_ptr(), cstr("Bo2Tts").as_ptr());
            if class.is_null() {
                return Err("no Bo2Tts class".into());
            }
            let speak = method_from_name(class, cstr("Speak").as_ptr(), 6);
            let last_error = method_from_name(class, cstr("LastError").as_ptr(), 0);
            if speak.is_null() || last_error.is_null() {
                return Err("Bo2Tts methods missing".into());
            }
            Ok(Self {
                api,
                domain,
                speak,
                last_error,
                _lib: lib,
            })
        }
    }

    /// A managed string or exception as text.
    unsafe fn text_of(&self, obj: Ptr) -> String {
        if obj.is_null() {
            return String::new();
        }
        // SAFETY: `obj` is a live managed object from the runtime.
        unsafe {
            let mut exc: Ptr = std::ptr::null_mut();
            let s = (self.api.object_to_string)(obj, &mut exc);
            if s.is_null() || !exc.is_null() {
                return "(unprintable)".into();
            }
            let raw = (self.api.string_to_utf8)(s);
            if raw.is_null() {
                return String::new();
            }
            let out = CStr::from_ptr(raw).to_string_lossy().into_owned();
            (self.api.free)(raw.cast());
            out
        }
    }

    /// One line in `voice`, as Klattersynth makes it.
    pub(crate) fn speak(&self, text: &str, voice: Voice) -> Result<Vec<f32>, String> {
        let text = cstr(text);
        let (mut freq, mut whisper, mut ms, mut flutter, mut speed) = (
            voice.base_frequency,
            u8::from(voice.whisper),
            voice.ms_per_speech_frame,
            voice.flutter,
            voice.flutter_speed,
        );
        // SAFETY: the arguments match `Bo2Tts.Speak(string, int, bool, int,
        // int, float)` (a string object, then pointers to the value types);
        // the result is a float[] whose elements are read within its length.
        unsafe {
            let s = (self.api.string_new)(self.domain, text.as_ptr());
            let mut args: [Ptr; 6] = [
                s,
                (&raw mut freq).cast(),
                (&raw mut whisper).cast(),
                (&raw mut ms).cast(),
                (&raw mut flutter).cast(),
                (&raw mut speed).cast(),
            ];
            let mut exc: Ptr = std::ptr::null_mut();
            let arr = (self.api.runtime_invoke)(self.speak, std::ptr::null_mut(), args.as_mut_ptr(), &mut exc);
            if !exc.is_null() {
                return Err(format!("Klattersynth threw: {}", self.text_of(exc)));
            }
            if arr.is_null() {
                return Err("no samples".into());
            }
            let n = (self.api.array_length)(arr);
            let first = (self.api.array_addr_with_size)(arr, 4, 0).cast::<f32>();
            let samples = if n == 0 || first.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(first, n).to_vec()
            };
            if samples.is_empty() {
                let mut exc: Ptr = std::ptr::null_mut();
                let err = (self.api.runtime_invoke)(self.last_error, std::ptr::null_mut(), std::ptr::null_mut(), &mut exc);
                return Err(format!("no samples ({})", self.text_of(err)));
            }
            Ok(samples)
        }
    }
}
