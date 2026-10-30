//! **Sized integers and floats**, as Meadow has them: `Int8`, `Int16`,
//! `Int32`, `UInt8`, `UInt16`, `UInt32`, `UInt64`, `Float` (an IEEE double)
//! and `Float32`. What each operation does on them is `meadow_rt::num`'s, so
//! that the reference interpreter and the runtimes agree: a sized integer
//! wraps at its width, its division rounds toward zero, and dividing by
//! zero is an error; a conversion to a sized integer keeps the low bits.
//!
//! A sized integer is held as its bits, zero-extended into a `u64`; what it
//! stands for is [`Width::value`].

/// A sized integer type other than `Int` itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum Width {
    I8,
    I16,
    I32,
    U8,
    U16,
    U32,
    U64,
}

impl Width {
    pub const ALL: [Width; 7] = [Width::I8, Width::I16, Width::I32, Width::U8, Width::U16, Width::U32, Width::U64];

    /// Its type's name: `Int8`.
    pub const fn name(self) -> &'static str {
        match self {
            Width::I8 => "Int8",
            Width::I16 => "Int16",
            Width::I32 => "Int32",
            Width::U8 => "UInt8",
            Width::U16 => "UInt16",
            Width::U32 => "UInt32",
            Width::U64 => "UInt64",
        }
    }

    pub const fn bits(self) -> u32 {
        match self {
            Width::I8 | Width::U8 => 8,
            Width::I16 | Width::U16 => 16,
            Width::I32 | Width::U32 => 32,
            Width::U64 => 64,
        }
    }

    pub const fn signed(self) -> bool {
        matches!(self, Width::I8 | Width::I16 | Width::I32)
    }

    pub const fn mask(self) -> u64 {
        if self.bits() == 64 { u64::MAX } else { (1u64 << self.bits()) - 1 }
    }

    /// `x` wrapped to this width: its low bits, which is how a value of the
    /// type is held.
    pub fn wrap(self, x: i128) -> u64 {
        (x as u64) & self.mask()
    }

    /// The number held bits `b` stand for: sign-extended for a signed width.
    pub fn value(self, b: u64) -> i128 {
        let b = b & self.mask();
        if self.signed() && (b >> (self.bits() - 1)) & 1 == 1 { b as i128 - (1i128 << self.bits()) } else { b as i128 }
    }
}

/// A number type a native works on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum NumTy {
    Int,
    Word(Width),
    Float,
    Float32,
}

impl NumTy {
    pub fn all() -> impl Iterator<Item = NumTy> {
        std::iter::once(NumTy::Int).chain(Width::ALL.map(NumTy::Word)).chain([NumTy::Float, NumTy::Float32])
    }

    /// Its type's name: `Int8`, `Float`.
    pub fn name(self) -> &'static str {
        match self {
            NumTy::Int => "Int",
            NumTy::Word(w) => w.name(),
            NumTy::Float => "Float",
            NumTy::Float32 => "Float32",
        }
    }

    pub fn is_float(self) -> bool {
        matches!(self, NumTy::Float | NumTy::Float32)
    }
}

/// What a numeric native does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum NumOp {
    Add,
    Sub,
    Mul,
    Div,
    /// an integer's remainder, with the dividend's sign
    Mod,
    Eq,
    Lt,
    /// as text, as Meadow's `display` writes it: `-3`, `1.5`, `2.0`
    Show,
    /// an integer as an `Int`, keeping its low 64 bits
    ToInt,
    /// an `Int` as this integer type, keeping its low bits
    FromInt,
    /// as a `Float`
    ToFloat,
    /// a `Float` as a `Float32`
    ToFloat32,
    /// a float, rounded toward negative infinity, as an `Int`
    Floor,
    /// an integer's bits, as Meadow's `bitAnd`, `bitOr`, `bitXor` and
    /// `bitNot` have them
    BitAnd,
    BitOr,
    BitXor,
    BitNot,
    /// shifted by an `Int` taken modulo the width: `shl` left, `shr`
    /// arithmetic on a signed type and logical on an unsigned one, `ushr`
    /// logical on both
    Shl,
    Shr,
    Ushr,
    /// how many bits are set, as an `Int`
    PopCount,
    /// its hash, Meadow's `hash` primitive (see [`crate::hash`])
    Hash,
}

impl NumOp {
    pub const ALL: [NumOp; 22] = [
        NumOp::Add,
        NumOp::Sub,
        NumOp::Mul,
        NumOp::Div,
        NumOp::Mod,
        NumOp::Eq,
        NumOp::Lt,
        NumOp::Show,
        NumOp::ToInt,
        NumOp::FromInt,
        NumOp::ToFloat,
        NumOp::ToFloat32,
        NumOp::Floor,
        NumOp::BitAnd,
        NumOp::BitOr,
        NumOp::BitXor,
        NumOp::BitNot,
        NumOp::Shl,
        NumOp::Shr,
        NumOp::Ushr,
        NumOp::PopCount,
        NumOp::Hash,
    ];

    fn is_bits(self) -> bool {
        matches!(
            self,
            NumOp::BitAnd | NumOp::BitOr | NumOp::BitXor | NumOp::BitNot | NumOp::Shl | NumOp::Shr | NumOp::Ushr | NumOp::PopCount
        )
    }

    fn suffix(self) -> &'static str {
        match self {
            NumOp::Add => "Add",
            NumOp::Sub => "Sub",
            NumOp::Mul => "Mul",
            NumOp::Div => "Div",
            NumOp::Mod => "Mod",
            NumOp::Eq => "Eq",
            NumOp::Lt => "Lt",
            NumOp::Show => "Show",
            NumOp::ToInt => "ToInt",
            NumOp::FromInt => "FromInt",
            NumOp::ToFloat => "ToFloat",
            NumOp::ToFloat32 => "ToFloat32",
            NumOp::Floor => "Floor",
            NumOp::BitAnd => "BitAnd",
            NumOp::BitOr => "BitOr",
            NumOp::BitXor => "BitXor",
            NumOp::BitNot => "BitNot",
            NumOp::Shl => "Shl",
            NumOp::Shr => "Shr",
            NumOp::Ushr => "Ushr",
            NumOp::PopCount => "PopCount",
            NumOp::Hash => "Hash",
        }
    }

    /// Whether there is a native for it at `ty`. `Int`'s arithmetic and
    /// comparisons are primitives of their own, and its text the prelude's.
    pub fn at(self, ty: NumTy) -> bool {
        match ty {
            _ if self == NumOp::Hash => true,
            NumTy::Int => self == NumOp::ToFloat || self.is_bits(),
            NumTy::Word(_) => !matches!(self, NumOp::ToFloat32 | NumOp::Floor),
            NumTy::Float if self.is_bits() => false,
            NumTy::Float32 if self.is_bits() => false,
            NumTy::Float => matches!(self, NumOp::Add | NumOp::Sub | NumOp::Mul | NumOp::Div | NumOp::Eq | NumOp::Lt | NumOp::Show | NumOp::ToFloat32 | NumOp::Floor),
            NumTy::Float32 => matches!(self, NumOp::Add | NumOp::Sub | NumOp::Mul | NumOp::Div | NumOp::Eq | NumOp::Lt | NumOp::Show | NumOp::ToFloat | NumOp::Floor),
        }
    }

    /// How many it takes.
    pub fn arity(self) -> usize {
        match self {
            NumOp::Add | NumOp::Sub | NumOp::Mul | NumOp::Div | NumOp::Mod | NumOp::Eq | NumOp::Lt => 2,
            NumOp::BitAnd | NumOp::BitOr | NumOp::BitXor | NumOp::Shl | NumOp::Shr | NumOp::Ushr => 2,
            _ => 1,
        }
    }

    /// The native's name: the type's, small, then the operation's:
    /// `int8Add`, `uint64FromInt`, `floatFloor`.
    pub fn name(self, ty: NumTy) -> String {
        format!("{}{}", ty.name().to_lowercase(), self.suffix())
    }

    /// The primitive of Meadow's runtimes it is, at `ty`.
    pub fn prim(self, ty: NumTy) -> String {
        let float = ty.is_float();
        match self {
            NumOp::Add if float => "addF".into(),
            NumOp::Sub if float => "subF".into(),
            NumOp::Mul if float => "mulF".into(),
            NumOp::Div if float => "divF".into(),
            NumOp::Lt if float => "ltF".into(),
            NumOp::Add => "add".into(),
            NumOp::Sub => "sub".into(),
            NumOp::Mul => "mul".into(),
            NumOp::Div => "div".into(),
            NumOp::Mod => "mod".into(),
            NumOp::Lt => "lt".into(),
            NumOp::Eq => "eq".into(),
            NumOp::Show => "display".into(),
            NumOp::ToInt => "toInt".into(),
            NumOp::FromInt => format!("to{}", ty.name()),
            NumOp::ToFloat => "toFloat".into(),
            NumOp::ToFloat32 => "toFloat32".into(),
            NumOp::Floor => "floor".into(),
            NumOp::BitAnd => "bitAnd".into(),
            NumOp::BitOr => "bitOr".into(),
            NumOp::BitXor => "bitXor".into(),
            NumOp::BitNot => "bitNot".into(),
            NumOp::Shl => "shl".into(),
            NumOp::Shr => "shr".into(),
            NumOp::Ushr => "ushr".into(),
            NumOp::PopCount => "popCount".into(),
            NumOp::Hash => "hash".into(),
        }
    }

    /// The type of what it takes, at `ty`.
    pub fn takes(self, ty: NumTy) -> NumTy {
        match self {
            NumOp::FromInt => NumTy::Int,
            _ => ty,
        }
    }

    /// The types of each thing it takes, at `ty`: a shift's amount is an
    /// `Int` whatever is shifted.
    pub fn takes_each(self, ty: NumTy) -> Vec<NumTy> {
        match self {
            NumOp::Shl | NumOp::Shr | NumOp::Ushr => vec![ty, NumTy::Int],
            _ => vec![self.takes(ty); self.arity()],
        }
    }

    /// What it gives, at `ty`: a number of this type, or a `Bool`, or text.
    pub fn gives(self, ty: NumTy) -> Gives {
        match self {
            NumOp::Add | NumOp::Sub | NumOp::Mul | NumOp::Div | NumOp::Mod | NumOp::FromInt => Gives::Num(ty),
            NumOp::BitAnd | NumOp::BitOr | NumOp::BitXor | NumOp::BitNot | NumOp::Shl | NumOp::Shr | NumOp::Ushr => Gives::Num(ty),
            NumOp::PopCount | NumOp::Hash => Gives::Num(NumTy::Int),
            NumOp::Eq | NumOp::Lt => Gives::Bool,
            NumOp::Show => Gives::Text,
            NumOp::ToInt | NumOp::Floor => Gives::Num(NumTy::Int),
            NumOp::ToFloat => Gives::Num(NumTy::Float),
            NumOp::ToFloat32 => Gives::Num(NumTy::Float32),
        }
    }
}

/// What a numeric native gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gives {
    Num(NumTy),
    Bool,
    Text,
}

/// A number, as the reference interpreter holds one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Number {
    Int(i64),
    Word(Width, u64),
    Float(f64),
    Float32(f32),
}

/// What a numeric native gave.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Num(Number),
    Bool(bool),
    Text(String),
}

/// A `Float` as Meadow writes one: always with a fraction, `2.0`.
pub fn fmt_float(x: f64) -> String {
    if x.is_finite() && x == x.trunc() { format!("{x:.1}") } else { format!("{x}") }
}

/// The same for a `Float32`, in its own shortest form.
pub fn fmt_float32(x: f32) -> String {
    if x.is_finite() && x == x.trunc() { format!("{x:.1}") } else { format!("{x}") }
}

impl std::fmt::Display for Number {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Number::Int(n) => write!(f, "{n}"),
            Number::Word(w, b) => write!(f, "{}", w.value(*b)),
            Number::Float(x) => f.write_str(&fmt_float(*x)),
            Number::Float32(x) => f.write_str(&fmt_float32(*x)),
        }
    }
}

impl Number {
    /// An integer's value, whatever its width.
    fn integer(self) -> Option<i128> {
        match self {
            Number::Int(n) => Some(n.into()),
            Number::Word(w, b) => Some(w.value(b)),
            _ => None,
        }
    }

    fn float(self) -> Option<f64> {
        match self {
            Number::Float(x) => Some(x),
            Number::Float32(x) => Some(x.into()),
            _ => None,
        }
    }
}

/// `op` at `ty` on `args`, or the error it stops the program with.
pub fn apply(op: NumOp, ty: NumTy, args: &[Number]) -> Result<Answer, String> {
    let wrong = || format!("`{}` was given {}", op.name(ty), args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(" and "));
    let a = *args.first().ok_or_else(wrong)?;
    let num = |n| Ok(Answer::Num(n));
    if op.is_bits() {
        return bits(op, ty, args).map(Answer::Num).ok_or_else(wrong);
    }
    match op {
        NumOp::Add | NumOp::Sub | NumOp::Mul | NumOp::Div | NumOp::Mod | NumOp::Eq | NumOp::Lt => {
            let b = *args.get(1).ok_or_else(wrong)?;
            match (ty, a, b) {
                (NumTy::Word(w), Number::Word(_, x), Number::Word(_, y)) => {
                    let (x, y) = (w.value(x), w.value(y));
                    let word = |n: i128| num(Number::Word(w, w.wrap(n)));
                    match op {
                        NumOp::Add => word(x.wrapping_add(y)),
                        NumOp::Sub => word(x.wrapping_sub(y)),
                        NumOp::Mul => word(x.wrapping_mul(y)),
                        NumOp::Div if y == 0 => Err("division by zero".into()),
                        NumOp::Div => word(x / y),
                        NumOp::Mod if y == 0 => Err("modulo by zero".into()),
                        NumOp::Mod => word(x % y),
                        NumOp::Eq => Ok(Answer::Bool(x == y)),
                        _ => Ok(Answer::Bool(x < y)),
                    }
                }
                (NumTy::Float, Number::Float(x), Number::Float(y)) => match op {
                    NumOp::Add => num(Number::Float(x + y)),
                    NumOp::Sub => num(Number::Float(x - y)),
                    NumOp::Mul => num(Number::Float(x * y)),
                    NumOp::Div => num(Number::Float(x / y)),
                    NumOp::Eq => Ok(Answer::Bool(x == y)),
                    NumOp::Lt => Ok(Answer::Bool(x < y)),
                    _ => Err(wrong()),
                },
                (NumTy::Float32, Number::Float32(x), Number::Float32(y)) => match op {
                    NumOp::Add => num(Number::Float32(x + y)),
                    NumOp::Sub => num(Number::Float32(x - y)),
                    NumOp::Mul => num(Number::Float32(x * y)),
                    NumOp::Div => num(Number::Float32(x / y)),
                    NumOp::Eq => Ok(Answer::Bool(x == y)),
                    NumOp::Lt => Ok(Answer::Bool(x < y)),
                    _ => Err(wrong()),
                },
                _ => Err(wrong()),
            }
        }
        NumOp::Show => Ok(Answer::Text(a.to_string())),
        NumOp::ToInt => a.integer().map(|v| Answer::Num(Number::Int(v as i64))).ok_or_else(wrong),
        NumOp::FromInt => match ty {
            NumTy::Word(w) => a.integer().map(|v| Answer::Num(Number::Word(w, w.wrap(v)))).ok_or_else(wrong),
            _ => Err(wrong()),
        },
        NumOp::ToFloat => match a {
            Number::Float32(x) => num(Number::Float(x.into())),
            _ => a.integer().map(|v| Answer::Num(Number::Float(v as f64))).ok_or_else(wrong),
        },
        NumOp::ToFloat32 => a.float().map(|x| Answer::Num(Number::Float32(x as f32))).ok_or_else(wrong),
        NumOp::Floor => {
            let x = a.float().ok_or_else(wrong)?;
            let f = x.floor();
            if !f.is_finite() {
                return Err(format!("`floor` of a non-finite Float: {x}"));
            }
            num(Number::Int(f as i64))
        }
        NumOp::Hash => num(Number::Int(crate::hash::number(a))),
        // (done above)
        NumOp::BitAnd | NumOp::BitOr | NumOp::BitXor | NumOp::BitNot | NumOp::Shl | NumOp::Shr | NumOp::Ushr | NumOp::PopCount => Err(wrong()),
    }
}

/// A bitwise native, as `meadow_rt::num::int_bits`, `int_not` and
/// `pop_count` compute it; `None` if it was given the wrong numbers.
fn bits(op: NumOp, ty: NumTy, args: &[Number]) -> Option<Number> {
    let amount = |n: &Number| match n {
        Number::Int(y) => Some(*y),
        _ => None,
    };
    match (ty, args) {
        (NumTy::Int, [Number::Int(x)]) => Some(Number::Int(match op {
            NumOp::BitNot => !x,
            NumOp::PopCount => i64::from(x.count_ones()),
            _ => return None,
        })),
        (NumTy::Int, [Number::Int(x), y]) => {
            let y = amount(y)?;
            Some(Number::Int(match op {
                NumOp::BitAnd => x & y,
                NumOp::BitOr => x | y,
                NumOp::BitXor => x ^ y,
                NumOp::Shl => x.wrapping_shl(y as u32),
                NumOp::Shr => x.wrapping_shr(y as u32),
                NumOp::Ushr => (*x as u64).wrapping_shr(y as u32) as i64,
                _ => return None,
            }))
        }
        (NumTy::Word(w), [Number::Word(_, x)]) => {
            let x = x & w.mask();
            match op {
                NumOp::BitNot => Some(Number::Word(w, !x & w.mask())),
                NumOp::PopCount => Some(Number::Int(i64::from(x.count_ones()))),
                _ => None,
            }
        }
        (NumTy::Word(w), [Number::Word(_, x), y]) => {
            let x = x & w.mask();
            let word = |y: &Number| match y {
                Number::Word(_, b) => Some(b & w.mask()),
                Number::Int(n) => Some(w.wrap(i128::from(*n))),
                _ => None,
            };
            let y = word(y)?;
            // (a shift amount is taken modulo the width, as Meadow's is)
            let s = (w.value(y) as u64 % u64::from(w.bits())) as u32;
            Some(Number::Word(
                w,
                match op {
                    NumOp::BitAnd => x & y,
                    NumOp::BitOr => x | y,
                    NumOp::BitXor => x ^ y,
                    NumOp::Shl => w.wrap(i128::from(x << s)),
                    NumOp::Shr if w.signed() => w.wrap(w.value(x) >> s),
                    NumOp::Shr | NumOp::Ushr => x >> s,
                    _ => return None,
                },
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sized_integers_wrap_and_floats_show_a_fraction() {
        let w = |n: i128| Number::Word(Width::I8, Width::I8.wrap(n));
        assert_eq!(apply(NumOp::Add, NumTy::Word(Width::I8), &[w(127), w(1)]), Ok(Answer::Num(w(-128))));
        assert_eq!(apply(NumOp::Show, NumTy::Word(Width::I8), &[w(-128)]), Ok(Answer::Text("-128".into())));
        assert_eq!(apply(NumOp::Div, NumTy::Word(Width::U8), &[w(1), Number::Word(Width::U8, 0)]), Err("division by zero".into()));
        assert_eq!(apply(NumOp::FromInt, NumTy::Word(Width::U8), &[Number::Int(-1)]), Ok(Answer::Num(Number::Word(Width::U8, 255))));
        assert_eq!(apply(NumOp::Show, NumTy::Float, &[Number::Float(2.0)]), Ok(Answer::Text("2.0".into())));
        assert_eq!(apply(NumOp::Floor, NumTy::Float, &[Number::Float(-1.5)]), Ok(Answer::Num(Number::Int(-2))));
        assert_eq!(NumOp::FromInt.name(NumTy::Word(Width::U64)), "uint64FromInt");
        assert_eq!(NumOp::FromInt.prim(NumTy::Word(Width::U64)), "toUInt64");
    }
}
