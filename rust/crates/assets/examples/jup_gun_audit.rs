//! Offline audit of every guest gun's first-person fit (Modern Warfare III,
//! II and (2019) guns through `jup_gun`):
//!
//! ```text
//! cargo run -p assets --example jup_gun_audit -- <zone dir> [gun filter]
//! ```
//!
//! `<zone dir>` holds the guns' part zones: decompressed `<zone>.zone` files
//! (as `fastfile_jup`'s `jup_gunfit --save` writes them), or the install's
//! `zone` folder with its `.ff` files (Windows, with the game's Oodle DLL
//! in the install root). A gun whose zones are absent is listed as not
//! audited. The report is Markdown: a summary of every measure, then one
//! table per game (docs/JUP_GUN_AUDIT.md is such a report).
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(dir) = args.next() else {
        eprintln!("usage: jup_gun_audit <zone dir> [gun filter]");
        return;
    };
    let filter = args.next();
    print!("{}", assets::jup_gun_audit(&PathBuf::from(dir), filter.as_deref()));
}
