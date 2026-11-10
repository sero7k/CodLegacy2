//! Disassembles one script given as its two dumped blocks (for example from the h1 `h1_scripts --dump`):
//! `gsc_file <stream-file> <code-file> [--hex]`
use gsc_iw6::{NameTable, Script, ScriptBytes};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let stream = std::fs::read(&a[0]).expect("stream");
    let code = std::fs::read(&a[1]).expect("code");
    let hex = a.iter().any(|x| x == "--hex");
    let bytes = ScriptBytes { stream: &stream, code: &code };
    match Script::parse(&bytes) {
        Ok(s) => print!("{}", gsc_iw6::render(&s, &code, &NameTable::known(), hex)),
        Err(e) => println!("decode error: {e}"),
    }
}
