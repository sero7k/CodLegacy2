//! A zone's script bundles' pairs: `t7_bundle <zone.ff> <substring>
//! [--dump <file>]` prints each matching bundle's pairs, script strings
//! resolved; `--dump` appends them to a file the sim's headless Black Ops
//! III test reads (`T7_BUNDLES`): per bundle a `B\t<name>` line, then per
//! pair `K\t<object, -1 for the bundle's own>\t<key>\t<kind>\t<value>\t<data>`.
fn main() {
    let mut a: Vec<String> = std::env::args().collect();
    let dump = a.iter().position(|x| x == "--dump").map(|i| {
        let f = a.get(i + 1).cloned().expect("--dump <file>");
        a.drain(i..i + 2);
        f
    });
    let mut out = String::new();
    let z = fastfile_t7::Zone::open(std::path::Path::new(&a[1])).expect("zone");
    let w = fastfile_t7::walk::walk(&z);
    let strings = &z.assets.script_strings;
    let ss = |i: u32| strings.get(i as usize).cloned().flatten().unwrap_or_else(|| format!("#{i}"));
    for ((ty, name), links) in &w.capture.links {
        if *ty != fastfile_t7::asset_type::AssetType::ScriptBundle || !name.contains(&a[2]) {
            continue;
        }
        println!("{name}");
        out.push_str(&format!("B\t{name}\n"));
        for (label, v) in links {
            let mut parts = label.split('#');
            let (who, key, kind) = (parts.next().unwrap_or(""), parts.next().and_then(|k| k.parse().ok()).unwrap_or(0), parts.next().unwrap_or(""));
            let object: i32 = who.strip_prefix("object").and_then(|n| n.parse().ok()).unwrap_or(-1);
            let (value, data) = match v {
                fastfile_t7::walk::capture::FieldValue::Str(t) if t.starts_with("ss:") => {
                    let mut w = t[3..].split(' ');
                    let n: u32 = w.next().and_then(|x| x.parse().ok()).unwrap_or(0);
                    let d = w.next().and_then(|x| x.strip_prefix("data:0x")).and_then(|x| u32::from_str_radix(x, 16).ok()).unwrap_or(0);
                    (strings.get(n as usize).cloned().flatten().unwrap_or_default(), d)
                }
                fastfile_t7::walk::capture::FieldValue::Asset { name, .. } => (name.clone(), 0),
                fastfile_t7::walk::capture::FieldValue::Str(t) => (t.clone(), 0),
            };
            out.push_str(&format!("K\t{object}\t{}\t{kind}\t{}\t{data}\n", ss(key), value.replace(['\t', '\n'], " ")));
            let v = match v {
                fastfile_t7::walk::capture::FieldValue::Str(t) if t.starts_with("ss:") => {
                    let n: u32 = t[3..].split(' ').next().and_then(|x| x.parse().ok()).unwrap_or(0);
                    format!("{} ({t})", ss(n))
                }
                other => format!("{other:?}"),
            };
            println!("  {who} {} [{kind}] = {v}", ss(key));
        }
    }
    if let Some(f) = dump {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(f).expect("dump file");
        file.write_all(out.as_bytes()).expect("write");
    }
}
