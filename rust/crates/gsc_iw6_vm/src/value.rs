//! Script values: the variable system's types, with the engine's reference semantics (arrays and
//! structs are shared by reference; strings compare case-insensitively).

use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, MutexGuard};

pub use gsc_iw6::Name;

/// A host entity handle (a game entity number the host understands).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId(pub u32);

/// A function of a loaded script: (index of the script in the `ScriptSet`, index of the function).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FuncRef {
    pub script: u32,
    pub func: u32,
}

/// An array key. Strings compare ASCII case-insensitively, like the engine's canonical strings.
#[derive(Clone, Debug)]
pub enum Key {
    Int(i32),
    Str(Arc<str>),
}

impl PartialEq for Key {
    fn eq(&self, o: &Key) -> bool {
        match (self, o) {
            (Key::Int(a), Key::Int(b)) => a == b,
            (Key::Str(a), Key::Str(b)) => a.eq_ignore_ascii_case(b),
            _ => false,
        }
    }
}
impl Eq for Key {}
impl Hash for Key {
    fn hash<H: Hasher>(&self, h: &mut H) {
        match self {
            Key::Int(i) => {
                0u8.hash(h);
                i.hash(h);
            }
            Key::Str(s) => {
                1u8.hash(h);
                for b in s.bytes() {
                    b.to_ascii_lowercase().hash(h);
                }
            }
        }
    }
}
impl Key {
    pub fn to_value(&self) -> Value {
        match self {
            Key::Int(i) => Value::Int(*i),
            Key::Str(s) => Value::Str(s.clone()),
        }
    }
    pub fn from_value(v: &Value) -> Option<Key> {
        match v {
            Value::Int(i) => Some(Key::Int(*i)),
            Value::Str(s) | Value::IStr(s) => Some(Key::Str(s.clone())),
            _ => None,
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Default, Debug)]
struct ArrayData {
    slots: Vec<Option<(Key, Value)>>,
    index: HashMap<Key, usize>,
    live: usize,
}

/// A script array (shared by reference). Elements keep their insertion order (INFERRED: the
/// engine lists a variable's children in a linked list; this VM iterates oldest first).
#[derive(Clone, Debug, Default)]
pub struct ArrRef(Arc<Mutex<ArrayData>>);

impl ArrRef {
    pub fn new() -> ArrRef {
        ArrRef::default()
    }
    pub fn from_values(v: impl IntoIterator<Item = Value>) -> ArrRef {
        let a = ArrRef::new();
        for x in v {
            a.push(x);
        }
        a
    }
    pub fn ptr_eq(&self, o: &ArrRef) -> bool {
        Arc::ptr_eq(&self.0, &o.0)
    }
    pub fn id(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }
    pub fn len(&self) -> usize {
        lock(&self.0).live
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn get(&self, k: &Key) -> Option<Value> {
        let d = lock(&self.0);
        d.index.get(k).and_then(|&i| d.slots[i].as_ref()).map(|(_, v)| v.clone())
    }
    pub fn set(&self, k: Key, v: Value) {
        let mut d = lock(&self.0);
        if let Some(&i) = d.index.get(&k) {
            if let Some(slot) = d.slots[i].as_mut() {
                slot.1 = v;
            }
            return;
        }
        let i = d.slots.len();
        d.slots.push(Some((k.clone(), v)));
        d.index.insert(k, i);
        d.live += 1;
    }
    pub fn remove(&self, k: &Key) -> Option<Value> {
        let mut d = lock(&self.0);
        let i = d.index.remove(k)?;
        let old = d.slots[i].take().map(|(_, v)| v);
        d.live -= 1;
        // Compact when more than half of the slots are holes.
        if d.slots.len() > 16 && d.live * 2 < d.slots.len() {
            let slots: Vec<_> = d.slots.drain(..).flatten().collect();
            d.index.clear();
            for (n, (k, _)) in slots.iter().enumerate() {
                d.index.insert(k.clone(), n);
            }
            d.slots = slots.into_iter().map(Some).collect();
        }
        old
    }
    /// `arr[arr.size] = v`: the next free integer index after the current size (the engine uses
    /// the element count as the key).
    pub fn push(&self, v: Value) {
        let mut n = self.len() as i32;
        while self.get(&Key::Int(n)).is_some() {
            n += 1;
        }
        self.set(Key::Int(n), v);
    }
    pub fn keys(&self) -> Vec<Key> {
        lock(&self.0).slots.iter().flatten().map(|(k, _)| k.clone()).collect()
    }
    pub fn values(&self) -> Vec<Value> {
        lock(&self.0).slots.iter().flatten().map(|(_, v)| v.clone()).collect()
    }
    pub fn entries(&self) -> Vec<(Key, Value)> {
        lock(&self.0).slots.iter().flatten().cloned().collect()
    }
    pub fn first_key(&self) -> Option<Key> {
        lock(&self.0).slots.iter().flatten().next().map(|(k, _)| k.clone())
    }
    /// The key after `k` in iteration order.
    pub fn next_key(&self, k: &Key) -> Option<Key> {
        let d = lock(&self.0);
        let i = *d.index.get(k)?;
        d.slots[i + 1..].iter().flatten().next().map(|(k, _)| k.clone())
    }
}

/// What kind of object a script `Object` is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjKind {
    Struct,
    Level,
    Anim,
}

#[derive(Debug)]
struct ObjectData {
    fields: Vec<(Name, Value)>,
}

/// A script object with named fields (a struct, `level` or `anim`), shared by reference.
#[derive(Clone, Debug)]
pub struct ObjRef {
    id: u64,
    kind: ObjKind,
    data: Arc<Mutex<ObjectData>>,
}

impl ObjRef {
    pub(crate) fn new(id: u64, kind: ObjKind) -> ObjRef {
        ObjRef { id, kind, data: Arc::new(Mutex::new(ObjectData { fields: Vec::new() })) }
    }
    /// A new empty struct (what `spawnstruct()` returns).
    pub fn new_struct() -> ObjRef {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1000);
        ObjRef::new(NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed), ObjKind::Struct)
    }
    /// A unique number (stable for the object's life).
    pub fn id(&self) -> u64 {
        self.id
    }
    pub fn kind(&self) -> ObjKind {
        self.kind
    }
    pub fn ptr_eq(&self, o: &ObjRef) -> bool {
        self.id == o.id
    }
    pub fn get(&self, n: &Name) -> Option<Value> {
        lock(&self.data).fields.iter().find(|(k, _)| k == n).map(|(_, v)| v.clone())
    }
    pub fn set(&self, n: Name, v: Value) {
        let mut d = lock(&self.data);
        if let Some(slot) = d.fields.iter_mut().find(|(k, _)| *k == n) {
            slot.1 = v;
        } else {
            d.fields.push((n, v));
        }
    }
    pub fn remove(&self, n: &Name) -> Option<Value> {
        let mut d = lock(&self.data);
        let i = d.fields.iter().position(|(k, _)| k == n)?;
        Some(d.fields.remove(i).1)
    }
    pub fn fields(&self) -> Vec<(Name, Value)> {
        lock(&self.data).fields.clone()
    }
}

/// `%anim` literal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnimRef {
    pub tree: String,
    pub name: String,
}

/// A script value.
#[derive(Clone, Debug)]
pub enum Value {
    Undefined,
    Int(i32),
    Float(f32),
    Str(Arc<str>),
    /// A localized string reference (`&"..."`).
    IStr(Arc<str>),
    Vector([f32; 3]),
    Entity(EntityId),
    Object(ObjRef),
    Array(ArrRef),
    Function(FuncRef),
    BuiltinFn(u16),
    BuiltinMethod(u16),
    Anim(Arc<AnimRef>),
    /// Internal: the call marker `pre_call` pushes.
    #[doc(hidden)]
    CallMarker,
    /// Internal: the frame base (a call marker after `check_params`, or a waittill marker).
    #[doc(hidden)]
    FrameBase,
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(Arc::from(s))
    }
    pub fn array(v: impl IntoIterator<Item = Value>) -> Value {
        Value::Array(ArrRef::from_values(v))
    }
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Undefined => "undefined",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "string",
            Value::IStr(_) => "localized string",
            Value::Vector(_) => "vector",
            Value::Entity(_) => "entity",
            Value::Object(_) => "object",
            Value::Array(_) => "array",
            Value::Function(_) => "function",
            Value::BuiltinFn(_) => "builtin function",
            Value::BuiltinMethod(_) => "builtin method",
            Value::Anim(_) => "animation",
            Value::CallMarker | Value::FrameBase => "<marker>",
        }
    }
    pub fn is_undefined(&self) -> bool {
        matches!(self, Value::Undefined)
    }
    pub fn as_int(&self) -> Option<i32> {
        match self {
            Value::Int(i) => Some(*i),
            Value::Float(f) => Some(*f as i32),
            _ => None,
        }
    }
    pub fn as_float(&self) -> Option<f32> {
        match self {
            Value::Int(i) => Some(*i as f32),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) | Value::IStr(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_vector(&self) -> Option<[f32; 3]> {
        match self {
            Value::Vector(v) => Some(*v),
            _ => None,
        }
    }
    pub fn as_entity(&self) -> Option<EntityId> {
        match self {
            Value::Entity(e) => Some(*e),
            _ => None,
        }
    }
    pub fn as_array(&self) -> Option<&ArrRef> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }
    /// The engine's `==` for two values of the same type (no coercion); arrays never compare.
    pub fn same(&self, o: &Value) -> bool {
        match (self, o) {
            (Value::Undefined, Value::Undefined) => true,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => (a - b).abs() < 1e-6,
            (Value::Str(a), Value::Str(b)) | (Value::IStr(a), Value::IStr(b)) => {
                a.eq_ignore_ascii_case(b)
            }
            (Value::Vector(a), Value::Vector(b)) => a == b,
            (Value::Entity(a), Value::Entity(b)) => a == b,
            (Value::Object(a), Value::Object(b)) => a.ptr_eq(b),
            (Value::Array(a), Value::Array(b)) => a.ptr_eq(b),
            (Value::Function(a), Value::Function(b)) => a == b,
            (Value::BuiltinFn(a), Value::BuiltinFn(b)) => a == b,
            (Value::BuiltinMethod(a), Value::BuiltinMethod(b)) => a == b,
            (Value::Anim(a), Value::Anim(b)) => a == b,
            _ => false,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Undefined => f.write_str("undefined"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Float(x) => f.write_str(&fmt_g(f64::from(*x))),
            Value::Str(s) => write!(f, "{s:?}"),
            Value::IStr(s) => write!(f, "&{s:?}"),
            Value::Vector(v) => write!(f, "({}, {}, {})", fmt_g(v[0].into()), fmt_g(v[1].into()), fmt_g(v[2].into())),
            Value::Entity(e) => write!(f, "entity#{}", e.0),
            Value::Object(o) => write!(f, "object#{}", o.id()),
            Value::Array(a) => write!(f, "array[{}]", a.len()),
            Value::Function(r) => write!(f, "&func({}.{})", r.script, r.func),
            Value::BuiltinFn(i) => write!(f, "&builtin({i:#x})"),
            Value::BuiltinMethod(i) => write!(f, "&method({i:#x})"),
            Value::Anim(a) => write!(f, "%{}", a.name),
            Value::CallMarker | Value::FrameBase => f.write_str("<marker>"),
        }
    }
}

/// C's `printf("%g")` (6 significant digits), as the engine formats floats in strings.
pub fn fmt_g(x: f64) -> String {
    if x.is_nan() {
        return if x.is_sign_negative() { "-nan" } else { "nan" }.to_owned();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-inf" } else { "inf" }.to_owned();
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0" } else { "0" }.to_owned();
    }
    let sci = format!("{:.5e}", x);
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    if !(-4..6).contains(&exp) {
        let mant = if mant.contains('.') { mant.trim_end_matches('0').trim_end_matches('.') } else { mant };
        let sign = if exp < 0 { '-' } else { '+' };
        return format!("{mant}e{sign}{:02}", exp.abs());
    }
    let decimals = (5 - exp).max(0) as usize;
    let s = format!("{x:.decimals$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s }
}

/// A vector as the engine prints it: `(x, y, z)`.
pub fn fmt_vector(v: [f32; 3]) -> String {
    format!("({}, {}, {})", fmt_g(v[0].into()), fmt_g(v[1].into()), fmt_g(v[2].into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g_format() {
        assert_eq!(fmt_g(0.5), "0.5");
        assert_eq!(fmt_g(100.0), "100");
        assert_eq!(fmt_g(1234567.0), "1.23457e+06");
        assert_eq!(fmt_g(0.0001), "0.0001");
        assert_eq!(fmt_g(0.00001), "1e-05");
        assert_eq!(fmt_g(-2.25), "-2.25");
        assert_eq!(fmt_g(1.33f32 as f64), "1.33");
    }

    #[test]
    fn array_order_and_remove() {
        let a = ArrRef::new();
        a.push(Value::Int(10));
        a.set(Key::Str("B".into()), Value::Int(20));
        a.push(Value::Int(30));
        assert_eq!(a.len(), 3);
        assert!(matches!(a.get(&Key::Str("b".into())), Some(Value::Int(20))));
        assert_eq!(a.first_key(), Some(Key::Int(0)));
        a.remove(&Key::Int(0));
        assert_eq!(a.first_key(), Some(Key::Str("B".into())));
        assert_eq!(a.next_key(&Key::Str("b".into())), Some(Key::Int(2)));
    }
}
