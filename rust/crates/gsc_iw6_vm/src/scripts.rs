//! The loaded scripts: file id -> decoded script, with the lookup tables the VM needs.

use std::collections::HashMap;

use gsc_iw6::{Name, Operand, Script, ScriptBytes};

use crate::value::{FuncRef, Value};
use std::sync::Arc;

/// A decoded script plus the tables the VM uses to run it.
pub struct Prepared {
    pub name: Name,
    pub script: Script,
    /// Code offset -> index in `script.instrs` (`u32::MAX` when not an instruction start).
    idx_of: Vec<u32>,
    func_by_name: HashMap<Name, u32>,
    /// Constant string values by instruction index.
    consts: HashMap<u32, Value>,
    /// Files this script includes (far-call targets), in first-use order.
    pub includes: Vec<Name>,
}

impl Prepared {
    fn new(name: Name, script: Script) -> Prepared {
        let mut idx_of = vec![u32::MAX; script.code_len + 1];
        let mut consts = HashMap::new();
        for (i, ins) in script.instrs.iter().enumerate() {
            if let Some(slot) = idx_of.get_mut(ins.offset) {
                *slot = i as u32;
            }
            if matches!(ins.op, 0x53 | 0x95)
                && let Some(Operand::Str(s)) = ins.operands.first()
            {
                let s: Arc<str> = Arc::from(s.as_str());
                consts.insert(i as u32, if ins.op == 0x53 { Value::Str(s) } else { Value::IStr(s) });
            }
        }
        let mut func_by_name = HashMap::new();
        for (i, f) in script.functions.iter().enumerate() {
            func_by_name.entry(f.name.clone()).or_insert(i as u32);
        }
        let includes = script.referenced_files();
        Prepared { name, script, idx_of, func_by_name, consts, includes }
    }

    /// Instruction index of the code offset.
    pub fn idx(&self, offset: usize) -> Option<usize> {
        match self.idx_of.get(offset) {
            Some(&i) if i != u32::MAX => Some(i as usize),
            _ => None,
        }
    }
    pub fn func_named(&self, n: &Name) -> Option<u32> {
        self.func_by_name.get(n).copied()
    }
    pub(crate) fn konst(&self, idx: usize) -> Option<&Value> {
        self.consts.get(&(idx as u32))
    }
    /// The function whose first instruction is at `offset`.
    pub fn func_at(&self, offset: usize) -> Option<u32> {
        self.script
            .functions
            .binary_search_by_key(&offset, |f| f.offset)
            .ok()
            .map(|i| i as u32)
    }
}

/// The scripts a VM can run, keyed by file name (a decimal file id, or a real name such as
/// `maps/mp/mp_frag`). Adding a script under an existing name replaces it (a patch zone's
/// version overrides the base zone's).
#[derive(Default)]
pub struct ScriptSet {
    scripts: Vec<Prepared>,
    by_name: HashMap<Name, u32>,
}

/// A name as zone scripts spell it: decimal -> `Name::Id`, else text.
pub fn file_name(s: &str) -> Name {
    match s.parse::<u16>() {
        Ok(n) if n != 0 => Name::Id(n),
        _ => Name::Text(s.to_owned()),
    }
}

impl ScriptSet {
    pub fn new() -> ScriptSet {
        ScriptSet::default()
    }

    /// Adds a decoded script; returns its index.
    pub fn add(&mut self, name: Name, script: Script) -> u32 {
        let p = Prepared::new(name.clone(), script);
        if let Some(&i) = self.by_name.get(&name) {
            self.scripts[i as usize] = p;
            i
        } else {
            let i = self.scripts.len() as u32;
            self.scripts.push(p);
            self.by_name.insert(name, i);
            i
        }
    }

    /// Decodes and adds the two blocks of a `ScriptFile` asset.
    pub fn add_bytes(&mut self, name: Name, stream: &[u8], code: &[u8]) -> Result<u32, gsc_iw6::Error> {
        let script = Script::parse(&ScriptBytes { stream, code })?;
        Ok(self.add(name, script))
    }

    pub fn len(&self) -> usize {
        self.scripts.len()
    }
    pub fn is_empty(&self) -> bool {
        self.scripts.is_empty()
    }
    pub fn get(&self, i: u32) -> Option<&Prepared> {
        self.scripts.get(i as usize)
    }
    pub fn index_of(&self, name: &Name) -> Option<u32> {
        self.by_name.get(name).copied()
    }
    pub fn iter(&self) -> impl Iterator<Item = (u32, &Prepared)> {
        self.scripts.iter().enumerate().map(|(i, p)| (i as u32, p))
    }

    /// Resolves `file::function`.
    pub fn resolve(&self, file: &Name, func: &Name) -> Option<FuncRef> {
        let s = *self.by_name.get(file)?;
        let f = self.scripts[s as usize].func_named(func)?;
        Some(FuncRef { script: s, func: f })
    }

    /// Included files that are not loaded: (script name, missing file).
    pub fn unresolved_includes(&self) -> Vec<(Name, Name)> {
        let mut out = Vec::new();
        for p in &self.scripts {
            for inc in &p.includes {
                if !self.by_name.contains_key(inc) {
                    out.push((p.name.clone(), inc.clone()));
                }
            }
        }
        out
    }

    /// `file::function` for messages.
    pub fn describe(&self, f: FuncRef) -> String {
        match self.scripts.get(f.script as usize) {
            Some(p) => {
                let fname = p.script.functions.get(f.func as usize).map(|x| x.name.to_string());
                format!("{}::{}", p.name, fname.unwrap_or_else(|| "?".to_owned()))
            }
            None => format!("?::{}", f.func),
        }
    }
}
