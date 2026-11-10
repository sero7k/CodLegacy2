//! Prints the opcode table as markdown rows: `gsc_ops`.
use gsc_iw6::opcodes::{Layout, OPS};

fn layout(l: Layout) -> &'static str {
    match l {
        Layout::None => "-",
        Layout::U8 => "u8",
        Layout::U16 => "u16",
        Layout::I32 => "i32",
        Layout::F32 => "f32",
        Layout::Vec3 => "3 x f32",
        Layout::Token => "u16 token (0xFFFF: inline name in stream)",
        Layout::Str => "u32 placeholder + string in stream",
        Layout::Anim => "8 bytes + tree and name strings in stream",
        Layout::AnimTree => "u8 + tree name string in stream",
        Layout::BuiltinId => "u16 builtin id",
        Layout::BuiltinArgc => "u8 argc, u16 builtin id",
        Layout::JumpFwd => "u16 forward distance",
        Layout::JumpBack => "u16 backward distance",
        Layout::Jump32 => "i32 distance",
        Layout::Switch => "i32 distance to the table",
        Layout::SwitchTable => "u16 n, n x (u32 case, 3-byte jump); strings in stream",
        Layout::CallFar => "3-byte placeholder; file and function tokens in stream",
        Layout::CallFarArgc => "3-byte placeholder, u8 argc; file and function tokens in stream",
        Layout::CallLocal => "3-byte distance (x4)",
        Layout::CallLocalArgc => "3-byte distance (x4), u8 argc",
    }
}

fn main() {
    for o in OPS {
        println!(
            "| 0x{:02x} | `{}` | {} | `{}` | {} |",
            o.code,
            o.mnemonic,
            layout(o.layout),
            o.stack,
            o.desc
        );
    }
}
