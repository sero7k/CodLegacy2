//! The bindings a program's `RDEF` declares: (name, input type, bind point).
//! Input types: 0 constant buffer, 2 texture, 3 sampler, 5 structured
//! buffer, 7 byte-address buffer.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub name: String,
    pub kind: u32,
    pub point: u32,
}

pub const CBUFFER: u32 = 0;
pub const TEXTURE: u32 = 2;
pub const SAMPLER: u32 = 3;

pub fn bindings(program: &[u8]) -> Vec<Binding> {
    let Some(r) = fastfile_t7::dxbc::chunk(program, b"RDEF") else { return Vec::new() };
    let u = |o: usize| r.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let s = |o: usize| r.get(o..).and_then(|b| b.iter().position(|&c| c == 0).map(|e| String::from_utf8_lossy(&b[..e]).into_owned()));
    let (Some(n), Some(at)) = (u(8), u(12)) else { return Vec::new() };
    (0..n.min(256) as usize)
        .filter_map(|i| {
            let b = at as usize + 32 * i;
            Some(Binding { name: s(u(b)? as usize)?, kind: u(b + 4)?, point: u(b + 20)? })
        })
        .collect()
}

/// The name of the constant buffer at a bind point.
pub fn cbuffer_name(b: &[Binding], point: u32) -> Option<&str> {
    b.iter().find(|x| x.kind == CBUFFER && x.point == point).map(|x| x.name.as_str())
}
