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
}

impl Prim {
    pub const ALL: [Prim; 7] =
        [Prim::IntAdd, Prim::IntMul, Prim::IntSub, Prim::IntLt, Prim::IntEq, Prim::IntDiv, Prim::IntMod];

    pub fn name(self) -> &'static str {
        match self {
            Prim::IntAdd => "+",
            Prim::IntMul => "*",
            Prim::IntSub => "-",
            Prim::IntDiv => "/",
            Prim::IntMod => "%",
            Prim::IntLt => "<",
            Prim::IntEq => "==",
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
        }
    }

    pub fn arity(self) -> usize {
        self.signature().0.len()
    }

    /// Binding strength when printed infix; all are left-associative.
    pub fn precedence(self) -> u8 {
        match self {
            Prim::IntAdd | Prim::IntSub => 6,
            Prim::IntMul | Prim::IntDiv | Prim::IntMod => 7,
            // below `::`, which is 5
            Prim::IntLt | Prim::IntEq => 4,
        }
    }
}

/// The first unique available for user-defined names.
pub const FIRST_USER_UNIQUE: usize = Builtin::COUNT + Prim::ALL.len();
