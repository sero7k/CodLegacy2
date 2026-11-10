//! Searches the inflated zone images for a text and prints each hit's
//! offset and a few bytes around it:
//! `cargo run -p fastfile_h1 --release --example h1_find_bytes -- <text> <zone.ff>...`.

fn main() {
    let mut args = std::env::args_os().skip(1);
    let text = args.next().expect("text").to_string_lossy().into_owned();
    let needle = text.as_bytes();
    for path in args {
        let path = std::path::PathBuf::from(path);
        let Ok(zone) = fastfile_h1::Zone::open(&path) else {
            eprintln!("{}: does not open", path.display());
            continue;
        };
        let image: &[u8] = &zone.image;
        let mut hits = 0;
        for (i, w) in image.windows(needle.len()).enumerate() {
            if w == needle {
                hits += 1;
                if hits <= 5 {
                    let a = i.saturating_sub(32);
                    let b = (i + needle.len() + 32).min(image.len());
                    println!(
                        "{} @{i:#x}: {}",
                        path.display(),
                        String::from_utf8_lossy(&image[a..b]).replace(|c: char| c.is_control(), ".")
                    );
                }
            }
        }
        if hits > 5 {
            println!("{}: {hits} hits", path.display());
        }
    }
}
