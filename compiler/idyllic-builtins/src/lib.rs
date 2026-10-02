//! Primitive constants that are in scope before any user code.
//!
//! Their uniques deliberately occupy the first name uniques, so the renamer
//! seeds its global scope with them and starts handing out fresh uniques at
//! [`FIRST_USER_UNIQUE`].  The type checker seeds its initial context with the
//! same uniques, which is what lets the two passes agree without threading a
//! table between them.

/// The primitive constants: closed types of sort `Type`, and `Div`, the one
/// effect that is built in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum Builtin {
    Unit,
    Int,
    Bool,
    Str,
    /// The type of grades.  Grades are ordinary (erased) terms, which is what
    /// lets a function be polymorphic in one: `{r : Grade} -> ..`.
    Grade,
    /// The type of effect rows, `{Log, Div | e}`, which are erased terms like
    /// grades: a function can be polymorphic in its effects, `{e : Effect}`.
    Effect,
    /// The effect of a computation that may not terminate.  Not an ordinary
    /// type: it has type `Effect`.
    Div,
}

impl Builtin {
    pub const ALL: [Builtin; 7] =
        [Builtin::Unit, Builtin::Int, Builtin::Bool, Builtin::Str, Builtin::Grade, Builtin::Effect, Builtin::Div];

    /// How many there are; primitives take the uniques after these.
    pub const COUNT: usize = Builtin::ALL.len();

    pub fn name(self) -> &'static str {
        match self {
            Builtin::Unit => "Unit",
            Builtin::Int => "Int",
            Builtin::Bool => "Bool",
            Builtin::Str => "String",
            Builtin::Grade => "Grade",
            Builtin::Effect => "Effect",
            Builtin::Div => "Div",
        }
    }

    /// The name unique reserved for this builtin.
    pub fn unique(self) -> usize {
        self as usize
    }
}

/// Primitive operations.  Like [`Builtin`]s they are names in the global
/// scope with reserved uniques, but they are *values*: each has a function
/// type over the primitive types, and computes once every argument is a
/// literal (see `Checker::reduce_prim`).
///
/// Where a `Grade` is expected the same operators are grade arithmetic
/// instead (see `Checker::check`); they are not overloaded anywhere else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum Prim {
    IntAdd,
    IntMul,
    IntSub,
    /// division, rounding toward zero
    IntDiv,
    /// the remainder of that, with the sign of the dividend
    IntMod,
    IntLt,
    IntEq,
    /// two strings, one after the other: `++`
    StrAppend,
}

impl Prim {
    pub const ALL: [Prim; 8] =
        [Prim::IntAdd, Prim::IntMul, Prim::IntSub, Prim::IntLt, Prim::IntEq, Prim::IntDiv, Prim::IntMod, Prim::StrAppend];

    pub fn name(self) -> &'static str {
        match self {
            Prim::IntAdd => "+",
            Prim::IntMul => "*",
            Prim::IntSub => "-",
            Prim::IntDiv => "/",
            Prim::IntMod => "%",
            Prim::IntLt => "<",
            Prim::IntEq => "==",
            Prim::StrAppend => "++",
        }
    }

    /// The name unique reserved for this primitive; they follow the builtins.
    pub fn unique(self) -> usize {
        Builtin::COUNT + self as usize
    }

    /// The primitive a resolved name refers to, if it is one.
    pub fn from_unique(unique: usize) -> Option<Prim> {
        Prim::ALL.into_iter().find(|p| p.unique() == unique)
    }

    /// Argument types and result type.
    pub fn signature(self) -> (&'static [Builtin], Builtin) {
        match self {
            Prim::IntAdd | Prim::IntMul | Prim::IntSub | Prim::IntDiv | Prim::IntMod => {
                (&[Builtin::Int, Builtin::Int], Builtin::Int)
            }
            Prim::IntLt | Prim::IntEq => (&[Builtin::Int, Builtin::Int], Builtin::Bool),
            Prim::StrAppend => (&[Builtin::Str, Builtin::Str], Builtin::Str),
        }
    }

    pub fn arity(self) -> usize {
        self.signature().0.len()
    }

    /// The primitive on two `Int`s.  An `Int` is 64 bits and wraps, the same
    /// in types, in the REPL and in native code, as it is the runtime's
    /// primitive integer.  `None` for a division by zero, which is an error
    /// at run time and stuck in a type.
    pub fn on_ints(self, a: i64, b: i64) -> Option<IntResult> {
        Some(match self {
            Prim::IntAdd => IntResult::Int(a.wrapping_add(b)),
            Prim::IntMul => IntResult::Int(a.wrapping_mul(b)),
            Prim::IntSub => IntResult::Int(a.wrapping_sub(b)),
            Prim::IntDiv | Prim::IntMod if b == 0 => return None,
            Prim::IntDiv => IntResult::Int(a.wrapping_div(b)),
            Prim::IntMod => IntResult::Int(a.wrapping_rem(b)),
            Prim::IntLt => IntResult::Bool(a < b),
            Prim::IntEq => IntResult::Bool(a == b),
            // (not on integers)
            Prim::StrAppend => return None,
        })
    }

    /// Binding strength when printed infix; all are left-associative.
    pub fn precedence(self) -> u8 {
        match self {
            Prim::IntAdd | Prim::IntSub => 6,
            Prim::IntMul | Prim::IntDiv | Prim::IntMod => 7,
            // as `::`, Haskell's `infixr 5`
            Prim::StrAppend => 5,
            // below `::`, which is 5
            Prim::IntLt | Prim::IntEq => 4,
        }
    }
}

/// What a primitive on `Int`s gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntResult {
    Int(i64),
    Bool(bool),
}

/// The first unique available for user-defined names.
pub const FIRST_USER_UNIQUE: usize = Builtin::COUNT + Prim::ALL.len();

/// What a `@native` declaration may stand for: a type or an operation that
/// every back end implements itself, declared in Idyll with its type
/// (`@native arrayLen : {a : Type} -> (xs : [0..1] Array a) -> Int`).  The
/// prelude declares `Array` and its operations, and `Std.Collections.MArray`
/// the mutable arrays.  A native's name is what identifies it, so a
/// declaration whose name is not one of these is an error.
///
/// **Arrays** are contiguous and persistent: an operation that changes one
/// gives a new array, and the old one is still there for whoever holds it.
/// That costs a copy only when someone does: an array nobody else refers to
/// is changed in place, which a back end tells by its reference count.
///
/// **Mutable arrays** are the same arrays, used linearly: every operation
/// takes the array and gives it back, so there is never anyone else holding
/// it, and every change is in place.  An element goes in unrestricted
/// (`[]`), since reading one leaves it in the array too, and so comes out
/// in a box, `a []`, for the reader to use as often as it likes.
///
/// An index outside an array is an error at run time, as dividing by zero
/// is: `Std.Collections.Array` gives the checked and the proof-carrying
/// ways to index.  Where a native gives back two things it is as a pair:
/// a data type whose first constructor has the two fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum Native {
    /// `Array : Type -> Type`
    Array,
    /// `{a : Type} -> (xs : [0..1] Array a) -> Int`
    ArrayLen,
    /// `{a : Type} -> (xs : [0..1] Array a) -> Int -> a`
    ArrayGet,
    /// `{a : Type} -> (d : [0..1] a) -> (xs : [0..1] Array a) -> Int -> a`: `d` outside it
    ArrayGetOr,
    /// `{a : Type} -> (xs : [0..1] Array a) -> Int -> a -> Array a`
    ArraySet,
    /// `{a : Type} -> Array a -> a -> Array a`: one more, at the end
    ArrayPush,
    /// `{a : Type} -> (xs : [0..1] Array a) -> Array a`: all but the last,
    /// of an array that has one (popping an empty array is an error)
    ArrayPop,
    /// `{a : Type} -> (xs : [0..1] Array a) -> Int -> Int -> Array a`: `[from, to)`, clamped
    ArraySlice,
    /// `{a : Type} -> Array a -> Array a -> Array a`
    ArrayConcat,
    /// `{a : Type} -> List a -> Array a`
    ArrayFromList,
    /// `{a : Type} -> Array a -> List a`
    ArrayToList,
    /// `MArray : Type -> Type`
    MArray,
    /// `{a : Type} -> Int -> (x : [] a) -> MArray a`: `n` copies of `x`
    MArrayNew,
    /// `{a : Type} -> MArray a -> Pair (Int []) (MArray a)`
    MArrayLen,
    /// `{a : Type} -> MArray a -> Int -> Pair (a []) (MArray a)`
    MArrayGet,
    /// `{a : Type} -> MArray a -> Int -> (x : [] a) -> MArray a`
    MArraySet,
    /// `{a : Type} -> MArray a -> Array a`
    MArrayFreeze,
    /// `{a : Type} -> (xs : [] Array a) -> MArray a`
    MArrayThaw,
}

impl Native {
    pub const ALL: [Native; 18] = [
        Native::Array,
        Native::ArrayLen,
        Native::ArrayGet,
        Native::ArrayGetOr,
        Native::ArraySet,
        Native::ArrayPush,
        Native::ArrayPop,
        Native::ArraySlice,
        Native::ArrayConcat,
        Native::ArrayFromList,
        Native::ArrayToList,
        Native::MArray,
        Native::MArrayNew,
        Native::MArrayLen,
        Native::MArrayGet,
        Native::MArraySet,
        Native::MArrayFreeze,
        Native::MArrayThaw,
    ];

    /// The name it is declared by.
    pub fn name(self) -> &'static str {
        match self {
            Native::Array => "Array",
            Native::ArrayLen => "arrayLen",
            Native::ArrayGet => "arrayGet",
            Native::ArrayGetOr => "arrayGetOr",
            Native::ArraySet => "arraySet",
            Native::ArrayPush => "arrayPush",
            Native::ArrayPop => "arrayPop",
            Native::ArraySlice => "arraySlice",
            Native::ArrayConcat => "arrayConcat",
            Native::ArrayFromList => "arrayFromList",
            Native::ArrayToList => "arrayToList",
            Native::MArray => "MArray",
            Native::MArrayNew => "marrayNew",
            Native::MArrayLen => "marrayLen",
            Native::MArrayGet => "marrayGet",
            Native::MArraySet => "marraySet",
            Native::MArrayFreeze => "marrayFreeze",
            Native::MArrayThaw => "marrayThaw",
        }
    }

    pub fn from_name(name: &str) -> Option<Native> {
        Native::ALL.into_iter().find(|n| n.name() == name)
    }

    /// A type constructor, rather than an operation: erased at run time.
    pub fn is_type(self) -> bool {
        matches!(self, Native::Array | Native::MArray)
    }

    /// How many arguments it takes at run time (after its erased ones).
    pub fn arity(self) -> usize {
        match self {
            Native::Array | Native::MArray => 0,
            Native::ArrayLen
            | Native::ArrayPop
            | Native::ArrayFromList
            | Native::ArrayToList
            | Native::MArrayLen
            | Native::MArrayFreeze
            | Native::MArrayThaw => 1,
            Native::ArrayGet | Native::ArrayPush | Native::ArrayConcat | Native::MArrayNew | Native::MArrayGet => 2,
            Native::ArrayGetOr | Native::ArraySet | Native::ArraySlice | Native::MArraySet => 3,
        }
    }
}
