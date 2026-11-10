//! Optional names for the ids scripts use.

use std::collections::HashMap;

use crate::Name;

/// Names for token ids (functions, fields), file ids and builtin ids. Built-in knowledge: `main`,
/// the builtin names read from the exe's handlers and the code-defined field ids (see
/// [`crate::builtins`]); the engine's full string table is not part of any script.
#[derive(Clone, Debug, Default)]
pub struct NameTable {
    pub tokens: HashMap<u16, String>,
    pub files: HashMap<u16, String>,
    pub builtins: HashMap<u16, String>,
}

impl NameTable {
    /// The few names the scripts themselves give away (INFERRED: the first function of a map's
    /// script is `main`, and is the callee of the `file::#20445` calls).
    pub fn known() -> NameTable {
        let mut t = NameTable::default();
        t.tokens.insert(20445, "main".to_owned());
        t.merge(include_str!("../data/builtins_iw6.names"));
        t.merge(include_str!("../data/fields_iw6.names"));
        t
    }

    /// Parses lines `token <id> <name>`, `file <id> <name>`, `builtin <id> <name>` (ids decimal
    /// or 0x-hex); `#` starts a comment.
    pub fn parse(text: &str) -> NameTable {
        let mut t = NameTable::default();
        t.merge(text);
        t
    }

    pub fn merge(&mut self, text: &str) {
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let mut it = line.split_whitespace();
            let (Some(kind), Some(id), Some(name)) = (it.next(), it.next(), it.next()) else {
                continue;
            };
            let id = match id.strip_prefix("0x") {
                Some(h) => u16::from_str_radix(h, 16).ok(),
                None => id.parse().ok(),
            };
            let Some(id) = id else { continue };
            let map = match kind {
                "token" => &mut self.tokens,
                "file" => &mut self.files,
                "builtin" => &mut self.builtins,
                _ => continue,
            };
            map.insert(id, name.to_owned());
        }
    }

    pub fn token(&self, n: &Name) -> String {
        match n {
            Name::Text(s) => s.clone(),
            Name::Id(i) => self
                .tokens
                .get(i)
                .map_or_else(|| format!("#{i}"), |s| s.clone()),
        }
    }

    pub fn file(&self, n: &Name) -> String {
        match n {
            Name::Text(s) => s.clone(),
            Name::Id(i) => self
                .files
                .get(i)
                .map_or_else(|| format!("{i}"), |s| s.clone()),
        }
    }

    pub fn builtin(&self, id: u16) -> String {
        let kind = if id >= 0x8000 { "method" } else { "function" };
        self.builtins
            .get(&id)
            .map_or_else(|| format!("{kind}#{id:#x}"), |s| s.clone())
    }
}
