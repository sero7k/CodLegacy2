//! Operators and conversions, as the engine's operator helpers do them.

use std::sync::Arc;

use crate::value::{fmt_g, fmt_vector, Value};

pub type R<T> = Result<T, String>;

fn mismatch(op: &str, a: &Value, b: &Value) -> String {
    format!("cannot apply {op} to {} and {}", a.type_name(), b.type_name())
}

/// The condition test of `if`, `||`, `&&`, `!`: ints and floats only (the engine raises a script
/// error for every other type, undefined included).
pub fn cond(v: &Value) -> R<bool> {
    match v {
        Value::Int(i) => Ok(*i != 0),
        Value::Float(f) => Ok(*f != 0.0),
        other => Err(format!("cannot cast {} to bool", other.type_name())),
    }
}

/// Mixed int/float pairs become float pairs; a vector with a scalar becomes a vector pair.
fn coerce(a: &Value, b: &Value) -> (Value, Value) {
    use Value::{Float, Int, Vector};
    match (a, b) {
        (Int(x), Float(_)) => (Float(*x as f32), b.clone()),
        (Float(_), Int(y)) => (a.clone(), Float(*y as f32)),
        (Vector(_), Int(y)) => (a.clone(), Vector([*y as f32; 3])),
        (Vector(_), Float(y)) => (a.clone(), Vector([*y; 3])),
        (Int(x), Vector(_)) => (Vector([*x as f32; 3]), b.clone()),
        (Float(x), Vector(_)) => (Vector([*x; 3]), b.clone()),
        _ => (a.clone(), b.clone()),
    }
}

fn to_text(v: &Value) -> Option<Arc<str>> {
    Some(match v {
        Value::Str(s) | Value::IStr(s) => s.clone(),
        Value::Int(i) => Arc::from(i.to_string().as_str()),
        Value::Float(f) => Arc::from(fmt_g(f64::from(*f)).as_str()),
        Value::Vector(v) => Arc::from(fmt_vector(*v).as_str()),
        _ => return None,
    })
}

/// `+`: numbers, vectors, and string concatenation (a string with an int, float or vector
/// converts the other side: ints as decimal, floats as C `%g`, vectors as `(x, y, z)`).
pub fn add(a: &Value, b: &Value) -> R<Value> {
    if matches!(a, Value::Str(_)) || matches!(b, Value::Str(_)) {
        if let (Some(x), Some(y)) = (to_text(a), to_text(b)) {
            if x.len() + y.len() + 1 > 0x2000 {
                return Err("string too long".to_owned());
            }
            let mut s = String::with_capacity(x.len() + y.len());
            s.push_str(&x);
            s.push_str(&y);
            return Ok(Value::Str(Arc::from(s.as_str())));
        }
        return Err(mismatch("+", a, b));
    }
    let (a2, b2) = coerce(a, b);
    match (&a2, &b2) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x.wrapping_add(*y))),
        (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x + y)),
        (Value::Vector(x), Value::Vector(y)) => Ok(Value::Vector([x[0] + y[0], x[1] + y[1], x[2] + y[2]])),
        _ => Err(mismatch("+", a, b)),
    }
}

pub fn sub(a: &Value, b: &Value) -> R<Value> {
    let (a2, b2) = coerce(a, b);
    match (&a2, &b2) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x.wrapping_sub(*y))),
        (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x - y)),
        (Value::Vector(x), Value::Vector(y)) => Ok(Value::Vector([x[0] - y[0], x[1] - y[1], x[2] - y[2]])),
        _ => Err(mismatch("-", a, b)),
    }
}

pub fn mul(a: &Value, b: &Value) -> R<Value> {
    let (a2, b2) = coerce(a, b);
    match (&a2, &b2) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x.wrapping_mul(*y))),
        (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x * y)),
        (Value::Vector(x), Value::Vector(y)) => Ok(Value::Vector([x[0] * y[0], x[1] * y[1], x[2] * y[2]])),
        _ => Err(mismatch("*", a, b)),
    }
}

/// `/`: int / int gives a float (as the engine does); a zero divisor is a script error.
pub fn div(a: &Value, b: &Value) -> R<Value> {
    let (a2, b2) = coerce(a, b);
    match (&a2, &b2) {
        (Value::Int(x), Value::Int(y)) => {
            if *y == 0 {
                Err("divide by 0".to_owned())
            } else {
                Ok(Value::Float(*x as f32 / *y as f32))
            }
        }
        (Value::Float(x), Value::Float(y)) => {
            if *y == 0.0 {
                Err("divide by 0".to_owned())
            } else {
                Ok(Value::Float(x / y))
            }
        }
        (Value::Vector(x), Value::Vector(y)) => {
            if y.contains(&0.0) {
                Err("divide by 0".to_owned())
            } else {
                Ok(Value::Vector([x[0] / y[0], x[1] / y[1], x[2] / y[2]]))
            }
        }
        _ => Err(mismatch("/", a, b)),
    }
}

fn ints(op: &str, a: &Value, b: &Value) -> R<(i32, i32)> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok((*x, *y)),
        _ => Err(mismatch(op, a, b)),
    }
}

pub fn rem(a: &Value, b: &Value) -> R<Value> {
    let (x, y) = ints("%", a, b)?;
    if y == 0 {
        return Err("divide by 0".to_owned());
    }
    Ok(Value::Int(x.wrapping_rem(y)))
}
pub fn bit_and(a: &Value, b: &Value) -> R<Value> {
    ints("&", a, b).map(|(x, y)| Value::Int(x & y))
}
pub fn bit_or(a: &Value, b: &Value) -> R<Value> {
    ints("|", a, b).map(|(x, y)| Value::Int(x | y))
}
pub fn bit_xor(a: &Value, b: &Value) -> R<Value> {
    ints("^", a, b).map(|(x, y)| Value::Int(x ^ y))
}
pub fn shl(a: &Value, b: &Value) -> R<Value> {
    ints("<<", a, b).map(|(x, y)| Value::Int(((x as u32) << (y as u32 & 31)) as i32))
}
pub fn shr(a: &Value, b: &Value) -> R<Value> {
    ints(">>", a, b).map(|(x, y)| Value::Int(x >> (y as u32 & 31)))
}
pub fn bit_not(a: &Value) -> R<Value> {
    match a {
        Value::Int(x) => Ok(Value::Int(!x)),
        _ => Err(format!("cannot apply ~ to {}", a.type_name())),
    }
}

/// Ordering comparisons: ints and floats only.
pub fn less(a: &Value, b: &Value) -> R<bool> {
    let (a2, b2) = coerce(a, b);
    match (&a2, &b2) {
        (Value::Int(x), Value::Int(y)) => Ok(x < y),
        (Value::Float(x), Value::Float(y)) => Ok(x < y),
        _ => Err(mismatch("<", a, b)),
    }
}
pub fn greater(a: &Value, b: &Value) -> R<bool> {
    let (a2, b2) = coerce(a, b);
    match (&a2, &b2) {
        (Value::Int(x), Value::Int(y)) => Ok(x > y),
        (Value::Float(x), Value::Float(y)) => Ok(x > y),
        _ => Err(mismatch(">", a, b)),
    }
}
/// `<=` is the negation of `>`, `>=` of `<` (as in the engine).
pub fn less_equal(a: &Value, b: &Value) -> R<bool> {
    greater(a, b).map(|g| !g)
}
pub fn greater_equal(a: &Value, b: &Value) -> R<bool> {
    less(a, b).map(|l| !l)
}

/// `==`: the two sides must be the same type after int/float and vector/scalar promotion
/// (undefined == undefined is true; anything else of different types is a script error, and so
/// is comparing arrays). Floats are equal within 1e-6; strings compare case-insensitively.
pub fn equal(a: &Value, b: &Value) -> R<bool> {
    let (a2, b2) = coerce(a, b);
    match (&a2, &b2) {
        (Value::Array(_), _) | (_, Value::Array(_)) => Err("cannot compare arrays".to_owned()),
        (Value::Str(_) | Value::IStr(_), Value::Str(_) | Value::IStr(_)) => Ok(a2.as_str() == b2.as_str() || a2.as_str().zip(b2.as_str()).is_some_and(|(x, y)| x.eq_ignore_ascii_case(y))),
        (Value::Undefined, Value::Undefined)
        | (Value::Int(_), Value::Int(_))
        | (Value::Float(_), Value::Float(_))
        | (Value::Vector(_), Value::Vector(_))
        | (Value::Entity(_), Value::Entity(_))
        | (Value::Object(_), Value::Object(_))
        | (Value::Function(_), Value::Function(_))
        | (Value::BuiltinFn(_), Value::BuiltinFn(_))
        | (Value::BuiltinMethod(_), Value::BuiltinMethod(_))
        | (Value::Anim(_), Value::Anim(_)) => Ok(a2.same(&b2)),
        (Value::Entity(_), Value::Object(_)) | (Value::Object(_), Value::Entity(_)) => Ok(false),
        _ => Err(mismatch("==", a, b)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules() {
        assert!(matches!(div(&Value::Int(3), &Value::Int(2)), Ok(Value::Float(f)) if f == 1.5));
        assert!(div(&Value::Int(3), &Value::Int(0)).is_err());
        assert!(matches!(mul(&Value::Vector([1.0, 2.0, 3.0]), &Value::Int(2)), Ok(Value::Vector(v)) if v == [2.0, 4.0, 6.0]));
        assert!(matches!(add(&Value::str("a"), &Value::Int(5)), Ok(Value::Str(s)) if &*s == "a5"));
        assert!(matches!(add(&Value::str("v"), &Value::Vector([1.0, 0.5, 0.0])), Ok(Value::Str(s)) if &*s == "v(1, 0.5, 0)"));
        assert!(equal(&Value::Undefined, &Value::Int(1)).is_err());
        assert_eq!(equal(&Value::str("ABC"), &Value::str("abc")).ok(), Some(true));
        assert_eq!(equal(&Value::Int(1), &Value::Float(1.0)).ok(), Some(true));
        assert!(cond(&Value::Undefined).is_err());
        assert_eq!(cond(&Value::Float(0.0)).ok(), Some(false));
        assert!(matches!(shl(&Value::Int(1), &Value::Int(33)), Ok(Value::Int(2))));
    }
}
