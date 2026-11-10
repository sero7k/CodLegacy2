//! Publisher tools (run on the owner's PC, never on the server):
//!
//!   codlegacy-publish keygen <secret-file>
//!       Makes a signing key pair. The secret goes in the file; the public key is printed and
//!       belongs in crates/player_launcher/update_key.pub before the launcher is built.
//!   codlegacy-publish sign <name> <build-dir> <version> <base-url> <secret-file>
//!       Writes <name>.json and <name>.json.sig in the current folder. Use name `manifest` for
//!       the game build and `launcher` for the launcher itself; upload both to /data/build/.

use std::path::Path;
use std::process::exit;

use codlegacy_launcher::{manifest, signing};

fn fail(msg: impl std::fmt::Display) -> ! {
    eprintln!("{msg}");
    exit(1)
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["keygen", file] => {
            if Path::new(file).exists() {
                fail(format!("{file} already exists; refusing to overwrite a signing key."));
            }
            let (secret, public) = signing::keygen().unwrap_or_else(|e| fail(e));
            std::fs::write(file, format!("{secret}\n")).unwrap_or_else(|e| fail(e));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600));
            }
            println!("Secret key written to {file}. Keep it private and back it up; do not commit it.");
            println!("Public key (put this in crates/player_launcher/update_key.pub):\n{public}");
        }
        ["sign", name, dir, version, base, secret_file] => {
            if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
                fail("name must be letters, digits, - or _");
            }
            let secret = std::fs::read_to_string(secret_file).unwrap_or_else(|e| fail(format!("{secret_file}: {e}")));
            let m = manifest::build(Path::new(dir), version, base).unwrap_or_else(|e| fail(e));
            let text = serde_json::to_string_pretty(&m).unwrap_or_else(|e| fail(e));
            let sig = signing::sign(&secret, text.as_bytes()).unwrap_or_else(|e| fail(e));
            std::fs::write(format!("{name}.json"), &text).unwrap_or_else(|e| fail(e));
            std::fs::write(format!("{name}.json.sig"), format!("{sig}\n")).unwrap_or_else(|e| fail(e));
            println!("Wrote {name}.json and {name}.json.sig ({} files, version {version}).", m.files.len());
        }
        _ => fail("usage:\n  codlegacy-publish keygen <secret-file>\n  codlegacy-publish sign <name> <build-dir> <version> <base-url> <secret-file>"),
    }
}
