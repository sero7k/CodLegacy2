//! The DXBC container: chunk directory and the input/output signatures.

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContainerError {
    NotDxbc,
    Truncated,
    NoShaderChunk,
}

/// One signature element (`ISGN` / `OSGN` / `PCSG`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SigElement {
    pub semantic: String,
    pub semantic_index: u32,
    /// D3D_NAME (0 undefined, 1 position, 6 vertex id, 8 instance id, 9 front face, 64 target, 65 depth, ...).
    pub system_value: u32,
    /// 1 uint, 2 sint, 3 float.
    pub component_type: u32,
    pub register: u32,
    pub mask: u8,
    /// Components the shader actually reads (inputs) or never writes (outputs).
    pub rw_mask: u8,
}

/// A parsed container: the shader token stream and its signatures.
#[derive(Clone, Debug, Default)]
pub struct Container<'a> {
    pub shader: &'a [u8],
    pub inputs: Vec<SigElement>,
    pub outputs: Vec<SigElement>,
    pub patch_constants: Vec<SigElement>,
}

fn u32_at(b: &[u8], at: usize) -> Result<u32, ContainerError> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or(ContainerError::Truncated)
}

fn signature(chunk: &[u8], stride: usize, lead: usize) -> Result<Vec<SigElement>, ContainerError> {
    let n = u32_at(chunk, 0)? as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let e = 8 + i * stride + lead;
        let name_at = u32_at(chunk, e)? as usize;
        let mut end = name_at;
        while chunk.get(end).copied().ok_or(ContainerError::Truncated)? != 0 {
            end += 1;
        }
        let semantic = String::from_utf8_lossy(&chunk[name_at..end]).into_owned();
        out.push(SigElement {
            semantic,
            semantic_index: u32_at(chunk, e + 4)?,
            system_value: u32_at(chunk, e + 8)?,
            component_type: u32_at(chunk, e + 12)?,
            register: u32_at(chunk, e + 16)?,
            mask: *chunk.get(e + 20).ok_or(ContainerError::Truncated)?,
            rw_mask: *chunk.get(e + 21).ok_or(ContainerError::Truncated)?,
        });
    }
    Ok(out)
}

impl<'a> Container<'a> {
    pub fn parse(b: &'a [u8]) -> Result<Self, ContainerError> {
        if b.get(0..4) != Some(b"DXBC") {
            return Err(ContainerError::NotDxbc);
        }
        let count = u32_at(b, 28)? as usize;
        let mut c = Container::default();
        let mut have_shader = false;
        for i in 0..count {
            let off = u32_at(b, 32 + 4 * i)? as usize;
            let tag = b.get(off..off + 4).ok_or(ContainerError::Truncated)?;
            let len = u32_at(b, off + 4)? as usize;
            let data = b
                .get(off + 8..off + 8 + len)
                .ok_or(ContainerError::Truncated)?;
            match tag {
                b"SHEX" | b"SHDR" => {
                    c.shader = data;
                    have_shader = true;
                }
                b"ISGN" => c.inputs = signature(data, 24, 0)?,
                b"OSGN" => c.outputs = signature(data, 24, 0)?,
                b"PCSG" => c.patch_constants = signature(data, 24, 0)?,
                b"ISG1" => c.inputs = signature(data, 32, 4)?,
                b"OSG1" => c.outputs = signature(data, 32, 4)?,
                b"OSG5" => c.outputs = signature(data, 28, 4)?,
                _ => {}
            }
        }
        if !have_shader {
            return Err(ContainerError::NoShaderChunk);
        }
        Ok(c)
    }
}
