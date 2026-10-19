//! A guided tour of the language, taken at the prompt, after Meadow's.
//!
//! `:tour` shows a step and then gets out of the way: the prompt afterwards
//! is the ordinary one, so anything can be typed, run and changed before
//! `:next` moves on.  A tutorial you read teaches you what someone else
//! typed; this is the thing itself, with the explanation beside it.
//!
//! Each step carries examples, and `:try` puts the next one *on the prompt*
//! rather than running it, so it can be read, edited, and then run.
//!
//! The tour grows with the language: each feature that lands gets a step,
//! in the section it belongs to (see ROADMAP.md).  The tests below replay
//! every example through the same session the REPL uses, in order, so a
//! step may build on the ones before it and no step can quietly go stale.

/// Which part of the tour a step belongs to.  Each is one stretch of the
/// tour, in this order, so that `:tour grades` means "start there".
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Section {
    /// Enough to write something.
    Basics,
    /// Grades: how many times things are used, and saying so.
    Grades,
    /// Types as values: universes, polymorphism, implicit arguments.
    Dependent,
    /// Equations, indexed families, and matching that teaches the types.
    Matching,
    /// Rows on arrows, divergence, and the program's own effects.
    Effects,
    /// Namespaces: constructors under their types, `use`, packages.
    Modules,
}

impl Section {
    pub const ALL: [Section; 6] = [
        Section::Basics,
        Section::Grades,
        Section::Dependent,
        Section::Matching,
        Section::Effects,
        Section::Modules,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Section::Basics => "the basics",
            Section::Grades => "grades",
            Section::Dependent => "dependent types",
            Section::Matching => "pattern matching",
            Section::Effects => "effects",
            Section::Modules => "modules",
        }
    }

    /// What `:tour <word>` answers to.  (Numbers are steps, not sections.)
    pub fn named(word: &str) -> Option<Section> {
        match word {
            "basics" | "basic" => Some(Section::Basics),
            "grades" | "grade" | "linearity" => Some(Section::Grades),
            "dependent" | "types" => Some(Section::Dependent),
            "matching" | "patterns" | "match" => Some(Section::Matching),
            "effects" | "effect" | "handlers" => Some(Section::Effects),
            "modules" | "module" | "use" | "packages" => Some(Section::Modules),
            _ => None,
        }
    }

    /// Where this section starts, if it has any steps yet.
    pub fn first(self) -> Option<usize> {
        STEPS.iter().position(|s| s.section == self)
    }
}

/// One step: what it is about, what it says, and what to type.
pub struct Step {
    pub title: &'static str,
    pub section: Section,
    /// Lines, already wrapped: a paragraph rewrapped by the terminal reads
    /// worse than one broken where it means to break.
    pub text: &'static [&'static str],
    /// What to type, in order; `:try` offers the next one each time.
    pub examples: &'static [Example],
}

/// Something to type.  An entry can span several lines, as at the prompt.
pub struct Example {
    pub source: &'static str,
    /// The headline of the error it is there to show, for an example that
    /// is meant to be rejected.  The tests hold it to exactly that error.
    pub rejected_with: Option<&'static str>,
}

const fn ok(source: &'static str) -> Example {
    Example { source, rejected_with: None }
}

const fn rejected(source: &'static str, headline: &'static str) -> Example {
    Example { source, rejected_with: Some(headline) }
}

pub const STEPS: &[Step] = &[
    // -----------------------------------------------------------------------
    // The basics
    // -----------------------------------------------------------------------
    Step {
        title: "Expressions",
        section: Section::Basics,
        text: &[
            "The prompt checks what you type, runs it, and answers with its",
            "value and its type, as `value : type`.",
            "",
            "An entry can span several lines, so Enter starts a new line; an",
            "empty line submits the entry.",
        ],
        examples: &[ok("1 + 2 * 3"), ok("\"hello\""), ok("10 - 3 < 5")],
    },
    Step {
        title: "Asking the REPL",
        section: Section::Basics,
        text: &[
            "`:t` checks an entry without running it. `:find` looks for a",
            "declaration by name, or by a type as Hoogle does: `:find Int -> String`",
            "finds `showInt`. Ctrl-F opens the same search over the prompt.",
            "",
            "`:prove` looks for a term of a type, and shows only what the checker",
            "accepts: `:prove {a : Type} -> a -> a` finds `\\x -> x`.",
            "",
            "`:ir` shows what a block of code is at every stage of the compiler,",
            "from tokens to Cut. Run one of these, then `:ir`.",
        ],
        examples: &[ok("showInt (6 * 7)"), ok("\\(x : Int) -> x + 1")],
    },
    Step {
        title: "Strings",
        section: Section::Basics,
        text: &[
            "`++` joins strings, and `showInt` writes an `Int` as one: the",
            "runtime prints only strings, so a number is shown by the",
            "program before it is printed.",
        ],
        examples: &[ok("\"forty\" ++ \"-\" ++ \"two\""), ok("\"n = \" ++ showInt (40 + 2)")],
    },
    Step {
        title: "Characters",
        section: Section::Basics,
        text: &[
            "A `Char`, `'a'`, is one Unicode character. A string is UTF-8:",
            "`stringByteLength` and `stringSlice` count its bytes, and",
            "`stringToChars` gives its characters. `Std.String` and",
            "`Std.Char` have the rest: splitting, trimming, `toInt`, `isDigit`.",
        ],
        examples: &[ok("charCode 'A'"), ok("stringToChars \"héllo\""), ok("stringSlice \"hello\" 1 3")],
    },
    Step {
        title: "Functions",
        section: Section::Basics,
        text: &[
            "A function is written `\\x -> body`, with the parameter's type in",
            "parentheses. Application is juxtaposition: `f x`, no brackets.",
            "",
            "A function has no value worth printing, so the prompt shows just",
            "its type.",
        ],
        examples: &[ok("(\\(x : Int) -> x * 2) 21"), ok("\\(x : Int) (y : Int) -> x + y")],
    },
    Step {
        title: "Definitions",
        section: Section::Basics,
        text: &[
            "At the top level, a definition is an equation with no keyword,",
            "as in Haskell, and its signature goes on a line of its own. A",
            "definition stays in scope for the entries after it, until",
            "`:reset`.",
        ],
        examples: &[
            ok("inc : Int -> Int\ninc n = n + 1"),
            ok("add : Int -> Int -> Int\nadd x y = x + y"),
            ok("add (inc 1) 40"),
        ],
    },
    Step {
        title: "Let and layout",
        section: Section::Basics,
        text: &[
            "`let` binds local names. Layout is significant, as in Haskell:",
            "the bindings of a `let` line up in a column, and `in` ends them.",
            "",
            "Only the first line shows the prompt, so a continuation line",
            "looks two columns further left than it is.",
        ],
        examples: &[ok("let x = 5 in x * 2"), ok("let x = 1\n    y = 2\n in x + y")],
    },
    Step {
        title: "Data types",
        section: Section::Basics,
        text: &[
            "`data` declares a type and its constructors, and `case` takes a",
            "value apart, one alternative per constructor. The alternatives",
            "line up, like a `let`'s bindings.",
            "",
            "A constructor lives under its type, `Op.Add`; `use Op.*` brings",
            "them all into scope bare.",
        ],
        examples: &[
            ok("data Op = Add | Sub
use Op.*"),
            ok("apply : Op -> Int -> Int -> Int\napply op x y = case op of\n  Add -> x + y\n  Sub -> x - y"),
            ok("apply Sub 10 3"),
            ok("if apply Add 1 1 == 2 then \"yes\" else \"no\""),
        ],
    },
    Step {
        title: "A constructor named for its type",
        section: Section::Basics,
        text: &[
            "A type with one constructor gives it the type's name. The bare",
            "name is then the type where a type is written, and the",
            "constructor in an expression or a pattern; `Point.Point` is always",
            "the constructor.",
        ],
        examples: &[
            ok("data Point = Point Int Int
use Point.*"),
            ok("mirror : Point -> Point\nmirror (Point x y) = Point y x"),
            ok("mirror (Point 1 2)"),
        ],
    },
    Step {
        title: "Lists and recursion",
        section: Section::Basics,
        text: &[
            "Lists are an ordinary data type; only the spelling is built in.",
            "`::` is infix, in expressions and in patterns.",
            "",
            "A recursive definition needs its signature, since there is",
            "nothing else to check its recursive calls against.",
        ],
        examples: &[
            ok("1 :: 2 :: 3 :: Nil"),
            ok("sum : List Int -> Int\nsum xs = case xs of\n  Nil -> 0\n  y :: ys -> y + sum ys"),
            ok("sum (1 :: 2 :: 3 :: Nil)"),
        ],
    },
    Step {
        title: "Tuples",
        section: Section::Basics,
        text: &[
            "`(1, True)` is a pair, the prelude's `Pair`, in a term, a pattern",
            "or a type. A longer tuple nests to the right: `(a, b, c)` is",
            "`(a, (b, c))`.",
        ],
        examples: &[
            ok("swap : (Int, Bool) -> (Bool, Int)\nswap (n, b) = (b, n)"),
            ok("swap (1, True)"),
        ],
    },
    Step {
        title: "Records",
        section: Section::Basics,
        text: &[
            "`record` declares a type with named fields: built with them in any",
            "order, read with `.`, updated with `{ p | x = .. }`, and matched by",
            "name. A field's type may mention the fields before it.",
        ],
        examples: &[
            ok("record Size = { width : Int, height : Int }"),
            ok("area : (s : [] Size) -> Int\narea s = s.width * s.height"),
            ok("taller : (s : [] Size) -> Size\ntaller s = { s | height = s.height * 2 }"),
            ok("area (taller (Size { width = 3, height = 4 }))"),
        ],
    },
    Step {
        title: "Arrays",
        section: Section::Basics,
        text: &[
            "`#[10; 20; 30]`, with semicolons, is an array: any element in",
            "constant time. `arrayGet`, `arraySet`, `arrayPush` and the rest",
            "are the prelude's natives. An array is persistent, setting an",
            "element gives a new one, and is changed in place when nothing",
            "else holds it. An index outside it stops the program.",
        ],
        examples: &[
            ok("#[10; 20; 30]"),
            ok("arraySet #[10; 20; 30] 1 99"),
            ok("arrayLen (arrayPush #[1; 2] 3)"),
        ],
    },
    // -----------------------------------------------------------------------
    // Linearity and grades
    // -----------------------------------------------------------------------
    Step {
        title: "Grades",
        section: Section::Grades,
        text: &[
            "Every binder has a grade: how many times it is used. One that",
            "says nothing may be used any number of times. A grade, in",
            "brackets before the type, says how many uses to expect instead:",
            "`[1]` is exactly once, linear, which is what lets a type describe",
            "a resource, such as memory that must be freed, and not just data.",
            "",
            "A module that starts with `@linear` makes `1` the default.",
        ],
        examples: &[
            ok("\\(x : Int) -> x + x"),
            rejected("\\(x : [1] Int) -> x + x", "usage mismatch"),
            ok("\\(x : [2] Int) -> x + x"),
            ok("\\(x : [0] Int) -> 5"),
            rejected("@linear\n\\(x : Int) -> x + x", "usage mismatch"),
        ],
    },
    Step {
        title: "Ranges",
        section: Section::Grades,
        text: &[
            "A grade can be a range. `0..1` is \"at most once\", `1..inf` is",
            "\"at least once\", and empty brackets mean any number of times,",
            "which is what a binder that says nothing has.",
            "A usage fits a grade when every count it might be is allowed.",
        ],
        examples: &[
            ok("\\(x : [0..1] Int) -> 7"),
            rejected("\\(x : [0..1] Int) -> x + x", "usage mismatch"),
            ok("\\(x : [] Int) -> x + x + x"),
        ],
    },
    Step {
        title: "Boxes",
        section: Section::Grades,
        text: &[
            "A grade on a parameter says how a function uses it. A box puts",
            "a grade on a value: `Int [2]` is an `Int` you may use twice.",
            "`[e]` makes one, and the pattern `[x]` opens one.",
            "",
            "`fromMaybe` only uses its default on one path, so it asks for a",
            "box it may leave unopened.",
        ],
        examples: &[
            ok("copy : Int [2] -> Int\ncopy [x] = x + x"),
            ok("copy [21]"),
            ok("data Maybe a = Nothing | Just a
use Maybe.*"),
            ok("fromMaybe : Int [0..1] -> Maybe Int -> Int\nfromMaybe [d] m = case m of\n  Nothing -> d\n  Just x -> x"),
            ok("fromMaybe [0] (Just 42)"),
            ok("fromMaybe [0] Nothing"),
        ],
    },
    Step {
        title: "Grade polymorphism",
        section: Section::Grades,
        text: &[
            "Grades are terms of type `Grade`, so a function can take one as",
            "an argument, in braces (the next section explains those). `app`",
            "works for a function of any grade, handed a box to match. It",
            "hands the box on once, which is what `@linear` says.",
        ],
        examples: &[
            ok("@linear\napp : {a b : Type} {r : Grade} (f : a [r] -> b) -> a [r] -> b\napp f [x] = f [x]"),
            ok("app copy [4]"),
            rejected("\\(y : [1] Int) -> app copy [y]", "usage mismatch"),
        ],
    },
    // -----------------------------------------------------------------------
    // Dependent types
    // -----------------------------------------------------------------------
    Step {
        title: "Types are values",
        section: Section::Dependent,
        text: &[
            "Types are ordinary terms, so they have types too. `Int` is a",
            "`Type`, but `Type` can't be one: that would let a program loop",
            "with no recursion in it (Girard's paradox). So universes form",
            "a tower, each in the next, and a type in one universe fits in",
            "every larger one.",
        ],
        examples: &[
            ok("Int"),
            ok("Type"),
            ok("(Int : Type 1)"),
            rejected("(Type : Type)", "type mismatch"),
        ],
    },
    Step {
        title: "Polymorphism",
        section: Section::Dependent,
        text: &[
            "Since types are values, a function can take one. The type is",
            "only used in other types, never computed with, so its grade is",
            "`0` and it is erased before the program runs.",
            "",
            "A function type over `Type` lives a universe up, in `Type 1`.",
        ],
        examples: &[
            ok("(a : [0] Type) -> a -> a"),
            ok("identity : (a : [0] Type) -> a -> a\nidentity a x = x"),
            ok("identity Int 3"),
            ok("identity Bool True"),
        ],
    },
    Step {
        title: "Implicit arguments",
        section: Section::Dependent,
        text: &[
            "An argument in braces is implicit: it is worked out from the",
            "others, so it isn't written at the call. Implicit arguments are",
            "erased unless they say otherwise.",
        ],
        examples: &[
            ok("id : {a : Type} -> a -> a\nid x = x"),
            ok("id 3"),
            ok("id \"implicitly a String\""),
        ],
    },
    Step {
        title: "Holes",
        section: Section::Dependent,
        text: &[
            "Write `_` to have the checker work something out. It says what",
            "it found, as a warning; if nothing determines a hole, that is",
            "an error, and it tells you the type the hole needs.",
        ],
        examples: &[ok("identity _ 3"), rejected("let n : Int = _ in n", "unsolved hole")],
    },
    Step {
        title: "Grades in types",
        section: Section::Dependent,
        text: &[
            "A result type that mentions an argument is using it too, so a",
            "binder has two grades, `(x : (s, r) a)`: `s` for the function,",
            "`r` for the type. The second defaults to any number.",
        ],
        examples: &[
            ok("(a : (0, 2) Type) -> (x : a) -> a"),
            rejected("(a : (0, 1) Type) -> (x : a) -> a", "usage mismatch"),
        ],
    },
    Step {
        title: "What a data type may hold",
        section: Section::Dependent,
        text: &[
            "A data type lives in `Type`, so its fields must be types in",
            "`Type`: one holding a `Type` would be a type of all types.",
            "",
            "And a type may appear in its own fields only to the right of",
            "every arrow. With `Bad (Bad -> Int)` you could apply what is",
            "inside a `Bad` to the `Bad` itself, and loop with no recursion",
            "anywhere. Inside another positive type, such as `List`, is fine.",
        ],
        examples: &[
            rejected("data U = U Type", "field too large for its data type"),
            rejected("data Bad = Bad (Bad -> Int)", "Bad is not strictly positive"),
            ok("data Rose a = Node a (List (Rose a))
use Rose.*"),
            ok("Node 1 (Node 2 Nil :: Nil)"),
        ],
    },
    // -----------------------------------------------------------------------
    // Pattern matching
    // -----------------------------------------------------------------------
    Step {
        title: "Equations",
        section: Section::Matching,
        text: &[
            "A function can be defined by several equations, tried in order.",
            "A pattern can be a literal, a variable, or `_` for anything.",
            "",
            "Matching on `n` looks at it, once for each literal it is tried",
            "against: `[]` lets `name` do that as often as it likes.",
        ],
        examples: &[ok("name : (n : [] Int) -> String
name 0 = \"none\"
name 1 = \"one\"
name _ = \"many\""), ok("name 1")],
    },
    Step {
        title: "Constructors, nested",
        section: Section::Matching,
        text: &[
            "Constructor patterns nest, and take their fields apart as deep",
            "as you like. An argument pattern with fields is parenthesised,",
            "as in Haskell.",
        ],
        examples: &[
            ok("data Nat = Z | S Nat
use Nat.*"),
            ok("half : (n : [] Nat) -> Nat
half (S (S n)) = S (half n)
half _ = Z"),
            ok("half (S (S (S (S (S Z)))))"),
        ],
    },
    Step {
        title: "Guards",
        section: Section::Matching,
        text: &[
            "`| condition = result` picks the first condition that holds;",
            "`otherwise` always does. If none holds, matching goes on to",
            "the next equation.",
        ],
        examples: &[
            ok("sign : (n : [] Int) -> Int
sign n
  | n < 0 = 0 - 1
  | n == 0 = 0
  | otherwise = 1"),
            ok("sign (0 - 5) + 10 * sign 7"),
        ],
    },
    Step {
        title: "Coverage",
        section: Section::Matching,
        text: &[
            "Every case must be covered. What is missing is said as the",
            "patterns you would write for it; an equation nothing can reach",
            "is pointed out too.",
        ],
        examples: &[rejected(
            "both : Bool -> Bool -> Int
both True True = 1
both False _ = 0",
            "the patterns do not cover every case",
        )],
    },
    Step {
        title: "Fields keep their grade",
        section: Section::Matching,
        text: &[
            "What comes out of a value has its grade. A linear `Nat` gives a",
            "linear field, which may not be dropped; one you may use at most",
            "once, `[0..1]`, gives fields you need not use.",
        ],
        examples: &[
            rejected("isZero : Nat -> Bool
isZero Z = True
isZero (S n) = False", "usage mismatch"),
            ok("isZero : (m : [0..1] Nat) -> Bool
isZero Z = True
isZero (S n) = False"),
            ok("isZero (S Z)"),
        ],
    },
    Step {
        title: "Sequences",
        section: Section::Matching,
        text: &[
            "`#[1, 2, 3]` is a list written out: `1 :: 2 :: 3 :: Nil`. (The",
            "`#` is because bare brackets are the grades'.) The same form is",
            "a pattern, of exactly that many elements.",
        ],
        examples: &[
            ok("#[1, 2, 3]"),
            ok("second : (xs : [] List Int) -> Int
second #[_, y] = y
second _ = 0"),
            ok("second #[10, 20]"),
        ],
    },
    Step {
        title: "Indexed families",
        section: Section::Matching,
        text: &[
            "`data T : kind where` gives each constructor its whole type, so",
            "each can say which `T` it builds: a vector's length is in its",
            "type. A variable a signature mentions but does not bind, like",
            "`n` and `a`, is bound for you, implicitly.",
            "",
            "`@seq` names the constructors a literal `#[..]` of the type is",
            "built with, so a literal gets its length into its type too.",
        ],
        examples: &[
            ok("@seq(nil = VNil, cons = VCons)
data Vec : Nat -> Type -> Type where
  VNil : Vec Z a
  VCons : a -> Vec n a -> Vec (S n) a
use Vec.*"),
            ok("VCons 1 (VCons 2 VNil)"),
            ok("(#[1, 2, 3] : Vec (S (S (S Z))) Int)"),
        ],
    },
    Step {
        title: "Matching teaches the types",
        section: Section::Matching,
        text: &[
            "Matching a vector of length `S n` against `VNil` would need",
            "`S n` to be `Z`, which it cannot be: so there is no `VNil` case",
            "to write. In the `VCons` case, the checker knows the length.",
        ],
        examples: &[
            ok("vhead : (v : [0..1] Vec (S n) a) -> a
vhead (VCons x xs) = x"),
            ok("vhead (VCons 7 VNil)"),
        ],
    },
    Step {
        title: "Computing in types",
        section: Section::Matching,
        text: &[
            "A recursive function that is sure to finish can compute in a",
            "type. `plus` recurs on a smaller `Nat` each time, so appending",
            "gives a vector as long as both.",
        ],
        examples: &[
            ok("plus : Nat -> Nat -> Nat
plus Z m = m
plus (S n) m = S (plus n m)"),
            ok("append : Vec n a -> Vec m a -> Vec (plus n m) a
append VNil ys = ys
append (VCons x xs) ys = VCons x (append xs ys)"),
            ok("append (VCons 1 VNil) (VCons 2 (VCons 3 VNil))"),
        ],
    },
    Step {
        title: "Proofs",
        section: Section::Matching,
        text: &[
            "An equality is a data type with one constructor, `Refl`, that",
            "only fits when both sides are the same. Matching on it teaches",
            "the checker they are, and a recursive function over proofs is",
            "a proof by induction.",
        ],
        examples: &[
            ok("data Eq : Nat -> Nat -> Type where
  Refl : Eq x x
use Eq.*"),
            ok("cong : Eq x y -> Eq (S x) (S y)
cong Refl = Refl"),
            ok("plusZero : (n : Nat) -> Eq (plus n Z) n
plusZero Z = Refl
plusZero (S k) = cong (plusZero k)"),
            ok("(Refl : Eq (plus (S Z) (S Z)) (S (S Z)))"),
        ],
    },
    Step {
        title: "Erased proofs",
        section: Section::Matching,
        text: &[
            "A proof is usually only there for the checker: given at grade",
            "`0`, it is erased. It can still be matched on when its type",
            "leaves it one constructor, since then there is nothing to look",
            "at. (That it really is `Refl` is safe to assume because erased",
            "code must be total: see the effects section.)",
        ],
        examples: &[
            ok("cast : (p : [0] Eq n m) -> Vec n a -> Vec m a
cast Refl v = v"),
            ok("cast Refl (VCons 1 VNil)"),
        ],
    },
    Step {
        title: "Impossible",
        section: Section::Matching,
        text: &[
            "`Fin n` is the numbers below `n`, so there is no `Fin Z`. An",
            "equation can say a case cannot happen with `impossible`, and",
            "the checker makes sure it is right.",
        ],
        examples: &[
            ok("data Fin : Nat -> Type where
  FZ : Fin (S n)
  FS : Fin n -> Fin (S n)
use Fin.*"),
            ok("noFin : Fin Z -> Int
noFin i impossible"),
            rejected("nope : Nat -> Int
nope n impossible", "not impossible"),
        ],
    },
    // -----------------------------------------------------------------------
    // Effects
    // -----------------------------------------------------------------------
    Step {
        title: "Rows",
        section: Section::Effects,
        text: &[
            "What a function does besides give a result goes after its type:",
            "`a -> b ! e`. The row `e` lists its effects; with none there is",
            "none, and the function is total.",
        ],
        examples: &[ok("Int -> Int ! Div"), ok("{e : Effect} -> (Int -> Int ! e) -> Int ! e")],
    },
    Step {
        title: "Div",
        section: Section::Effects,
        text: &[
            "`Div` is the effect of maybe never finishing. A recursion whose",
            "argument does not get smaller each time round, as the checker",
            "sees it, has to say `! Div`. It runs all the same.",
            "",
            "Or `@terminating`, on the line before the signature, vouches that",
            "it ends: a promise nothing checks.",
        ],
        examples: &[
            rejected(
                "count : (n : [] Int) -> Int
count n = if n == 0 then 0 else count (n - 1)",
                "count may not terminate",
            ),
            ok("count : (n : [] Int) -> Int ! Div
count n = if n == 0 then 0 else count (n - 1)"),
            ok("count 3"),
            ok("@terminating
countdown : (n : [] Int) -> Int
countdown n = if n < 1 then 0 else countdown (n - 1)"),
            ok("countdown 3"),
        ],
    },
    Step {
        title: "Types are total",
        section: Section::Effects,
        text: &[
            "Nothing with effects gets into a type, where it could not keep",
            "its promise: a `count 1` in a type might never be finished.",
            "Nor into an erased argument, which is never run.",
        ],
        examples: &[
            ok("data IEq : Int -> Int -> Type where
  IRefl : IEq x x
use IEq.*"),
            rejected("(IRefl : IEq (count 1) 0)", "a type must be pure"),
            rejected("(\\(n : [0] Int) -> 1) (count 5)", "an erased argument must be pure"),
        ],
    },
    Step {
        title: "Your own effects",
        section: Section::Effects,
        text: &[
            "`effect` declares an effect and its operations. Calling one",
            "performs it; `handle` says what that does, and `k` in a clause",
            "is the rest of the computation, to go on with.",
        ],
        examples: &[
            ok("effect Ask where
  ask : () -> Int"),
            ok("sum3 : () -> Int ! Ask
sum3 u = ask () + ask () + ask ()"),
            ok("handle sum3 () with
  ask _ k -> k 10"),
        ],
    },
    Step {
        title: "Not going on",
        section: Section::Effects,
        text: &[
            "A clause that never uses `k` is an exception: what it gives is",
            "what the whole `handle` gives. `return` says what becomes of",
            "the value when there is no exception.",
        ],
        examples: &[
            ok("effect Fail where
  fail : () -> Int"),
            ok("safeDiv : (a : [] Int) -> (b : [] Int) -> Int ! Fail
safeDiv a b = if b == 0 then fail () else a / b"),
            ok("handle safeDiv 7 0 with
  return x -> x
  fail _ k -> 0"),
        ],
    },
    Step {
        title: "Going on twice",
        section: Section::Effects,
        text: &[
            "`k` can be used more than once: here, to try both answers. How",
            "often a clause resumes is `k`'s grade, worked out from the",
            "clause itself. More than once needs the operation to allow it,",
            "with `@many`. Unmarked, an operation is resumed at most once,",
            "which is what lets it run on a stack.",
        ],
        examples: &[
            ok("effect Choose where
  @many flip : () -> Bool"),
            ok("pick : () -> Int ! Choose
pick u = if flip () then 1 else 10"),
            ok("handle pick () with
  flip _ k -> k True + k False"),
        ],
    },
    Step {
        title: "Polymorphic effects",
        section: Section::Effects,
        text: &[
            "A row variable, bound for you like any other, lets a function",
            "take whatever effects the functions it is given have.",
        ],
        examples: &[
            ok("twice : (f : [] (() -> Int ! e)) -> Int ! e
twice f = f () + f ()"),
            ok("handle twice sum3 with
  ask _ k -> k 1"),
        ],
    },
    Step {
        title: "Different effects, one row",
        section: Section::Effects,
        text: &[
            "One row variable is enough for a function that runs two others:",
            "`compose f g` does what each does, and a function with fewer",
            "effects fits where more are allowed. `sum3` asks and `noisy`",
            "logs, so composing them is at `{Ask, Log}`.",
        ],
        examples: &[
            ok("effect Log where
  log : () -> ()"),
            ok("noisy : Int -> Int ! Log
noisy n = let u = log () in n"),
            ok("compose : (f : b -> c ! e) -> (g : a -> b ! e) -> (x : a) -> c ! e
compose f g x = f (g x)"),
            ok("both u = compose noisy sum3 u"),
            ok("handle (handle both () with { ask _ k -> k 1 }) with { log _ k -> k () }"),
        ],
    },
    Step {
        title: "Resumed more than once",
        section: Section::Effects,
        text: &[
            "A row variable's kind is `Effect`: effects resumed at most once,",
            "which lets code polymorphic in it keep its continuations on a",
            "stack. `Choose`, with an `@many` operation, is `Effect []`, so",
            "`twice` refuses it. A function that takes such effects binds",
            "its row variable at `Effect []`, and takes the others too.",
        ],
        examples: &[
            rejected(
                "handle twice pick with
  flip _ k -> k True + k False",
                "these effects may be resumed more than once",
            ),
            ok("twiceM : {e : Effect []} -> (f : [] (() -> Int ! e)) -> Int ! e
twiceM f = f () + f ()"),
            ok("handle twiceM pick with
  flip _ k -> k True + k False"),
        ],
    },
    Step {
        title: "Naming a row",
        section: Section::Effects,
        text: &[
            "`effect Asking = {Ask, Div}` names a row, to use wherever an",
            "effect goes. It is a definition like any other, of a term of",
            "type `Effect`.",
        ],
        examples: &[
            ok("effect Asking = {Ask, Div}"),
            ok("sum6 : () -> Int ! Asking
sum6 u = sum3 () + sum3 ()"),
            ok("handle sum6 () with
  ask _ k -> k 2"),
        ],
    },
    Step {
        title: "The world",
        section: Section::Effects,
        text: &[
            "The runtime handles the effects in `Eff`, as Meadow's does:",
            "`Console`, `Fs`, `Process`, `Random`, `Time`, `Test` and `Div`.",
            "An operation of theirs nothing else handles, it does. (The",
            "tutorial lists them all.)",
            "",
            "`Console` only writes strings, with `putStr`; `putStrLn` and",
            "`printInt` are prelude functions over it.",
        ],
        examples: &[
            ok("putStrLn \"hello\""),
            ok("intBetween 1 7"),
            ok("exists \"README.md\""),
        ],
    },
    Step {
        title: "Loaded dice",
        section: Section::Effects,
        text: &[
            "The world's effects are effects like any other, so a handler of",
            "your own can take one: here, to test code that rolls dice with",
            "dice that always land in the middle. An operation takes as many",
            "arguments as its type has arrows, and `intBetween` takes two.",
        ],
        examples: &[
            ok("dice : () -> Int ! Random
dice () = intBetween 1 7"),
            ok("handle dice () + dice () with
  nextInt _ k -> k 4
  intBetween lo hi k -> k ((lo + hi) / 2)
  nextSeed _ k -> k 0"),
        ],
    },
    Step {
        title: "Giving up",
        section: Section::Effects,
        text: &[
            "`abort` gives up, with a message. It is an effect, `Abort`, so",
            "a function that may give up says so, and a handler can carry",
            "on instead. Unhandled, it writes the message to standard error",
            "and exits with status 1.",
        ],
        examples: &[
            ok("halve : (n : [] Int) -> Int ! Abort
halve n = if n % 2 == 0 then n / 2 else abort \"odd\""),
            ok("handle halve 7 with
  abort m k -> let u = putErr m in 0"),
        ],
    },
    Step {
        title: "main",
        section: Section::Effects,
        text: &[
            "A program with a `main` runs it: `main : () -> () ! Eff` may do",
            "what the runtime handles, and nothing else, which is checked",
            "before it runs. Defining one here doesn't run it; `main ()`",
            "does.",
        ],
        examples: &[
            ok("main : () -> () ! Eff
main () = let u = putStrLn \"Hello, Idyll!\" in printInt (6 * 7)"),
            ok("main ()"),
        ],
    },
    // -----------------------------------------------------------------------
    // Modules
    // -----------------------------------------------------------------------
    Step {
        title: "Under their types",
        section: Section::Modules,
        text: &[
            "A constructor lives under its type, `Suit.Hearts`, even in the",
            "module that declares it, so two types may share a constructor's",
            "name. `use Suit.*` brings them all into scope bare, and",
            "`use Suit (Spades)` just the ones named.",
        ],
        examples: &[
            ok("data Suit = Hearts | Spades"),
            rejected("Hearts", "name not in scope: Hearts"),
            ok("Suit.Hearts"),
            ok("use Suit (Spades)"),
            ok("Spades"),
        ],
    },
    Step {
        title: "Packages",
        section: Section::Modules,
        text: &[
            "A package is a directory with `src/`: `src/Main.idl` is its root,",
            "and `src/Math.idl` is the module `Math`, which another module",
            "reaches with `use App.Math (double)`, `use Math as M` (then",
            "`M.double`), or `App.Math.double` written in full. A name is",
            "private to its module until it says `@pub(pkg)`, or `@pub` for",
            "other packages too. The REPL is one module; `idyll run dir`",
            "builds and runs a package. Here, the prelude's own `List`, in full:",
        ],
        examples: &[ok("List.Nil : List Int"), ok("Maybe.Just (List.Nil : List Int)")],
    },
    Step {
        title: "Dependencies",
        section: Section::Modules,
        text: &[
            "`idyll init` makes a package and `idyll run` runs it, as cargo",
            "does. Its `Idyll.toml` names what it depends on, `Util = { path =",
            "\"../util\" }` or `Json = { git = \"...\", version = \"1.2.0\" }`,",
            "and `idyll.lock` pins each git dependency's commit until",
            "`idyll update`. A package is checked against the compiled ones it",
            "uses, and only what they make `@pub` crosses, bodies and all: a",
            "library's function computes in your types as your own would.",
            "Unchanged packages are not compiled again. A library offers:",
        ],
        examples: &[ok("@pub\nquadruple : Int -> Int\nquadruple n = n * 4"), ok("quadruple 10")],
    },
    Step {
        title: "The standard library",
        section: Section::Modules,
        text: &[
            "Every package has `Std`, the standard library, and needs no line in",
            "`Idyll.toml` for it; nothing of it is in scope until a `use` brings",
            "it. Laid out as Meadow's is: `Std.Collections` has lists, `Vec`s",
            "whose length is in their type, arrays, mutable arrays and Meadow's",
            "RRB `Vector`; `Std.Num` has integers, naturals with proofs about",
            "them, and `Fin` indices; and there are equality, `Maybe`, `Result`,",
            "`Either`, pairs and dependent pairs, sorting, strings, files, and",
            "effects for exceptions, state and generators. `:find Std.` lists it",
            "all.",
        ],
        examples: &[
            ok("use Std.Collections.List (map, sum)"),
            ok("sum (map (\\(x : Int) -> x * 2) #[1, 2, 3])"),
            ok("use Std.Collections.Vec (Vec, fromList, index)\nuse Std.Num.Fin.Fin.*"),
            ok("index (FS FZ) (fromList #[10, 20, 30])"),
            ok("use Std.Collections.Vector (Vector, pushBack, pushFront, empty, toList)"),
            ok("toList (pushFront (pushBack (pushBack empty 2) 3) 1)"),
        ],
    },
];

/// Where a step is in the tour, for its heading.
pub fn heading(at: usize) -> String {
    format!("Step {} of {}: {}", at + 1, STEPS.len(), STEPS[at].title)
}

#[cfg(test)]
mod tests {
    use super::*;
    use idyllic::pipeline::{Evaluation, Options, Session};
    use idyllic_diagnostics::OutputStyle;

    /// Every example, entered in the order the tour gives them, which is the
    /// order someone taking it would type them.  A tour whose examples do
    /// not work teaches, first of all, that the thing in front of you is
    /// broken.
    #[test]
    fn every_example_does_what_the_tour_says() {
        let opts = Options { debug: false, style: OutputStyle::PLAIN, live: false };
        let mut session = Session::default();
        for (i, step) in STEPS.iter().enumerate() {
            for ex in step.examples {
                let where_ = format!("step {} ({}): `{}`", i + 1, step.title, ex.source);
                let out = session.enter(ex.source, &opts).output;
                let errors: Vec<&str> =
                    out.diagnostics.iter().filter(|d| d.is_error()).map(|d| d.message.as_str()).collect();
                match ex.rejected_with {
                    None => {
                        assert!(errors.is_empty(), "{where_} has errors: {errors:?}");
                        assert!(
                            !matches!(out.evaluation, Some(Evaluation::Failed(_))),
                            "{where_} failed at run time: {:?}",
                            out.evaluation
                        );
                    }
                    Some(headline) => assert_eq!(errors, [headline], "{where_}"),
                }
            }
        }
    }

    /// `:tour grades` starts a section in whatever session you have, perhaps
    /// a fresh one, so a section may use only what it defines itself.
    #[test]
    fn each_section_stands_alone() {
        let opts = Options { debug: false, style: OutputStyle::PLAIN, live: false };
        for section in Section::ALL {
            let mut session = Session::default();
            for step in STEPS.iter().filter(|s| s.section == section) {
                for ex in step.examples.iter().filter(|ex| ex.rejected_with.is_none()) {
                    let out = session.enter(ex.source, &opts).output;
                    let errors: Vec<&str> =
                        out.diagnostics.iter().filter(|d| d.is_error()).map(|d| d.message.as_str()).collect();
                    assert!(
                        errors.is_empty(),
                        "`:tour {}`, then `{}`: {errors:?} (does it use a name from another section?)",
                        section.title(),
                        ex.source
                    );
                }
            }
        }
    }

    /// A step ends on something to look at: a value, or the error it is
    /// there to show.  (An entry of declarations only reports their types.)
    #[test]
    fn every_step_ends_on_something_to_run() {
        let opts = Options { debug: false, style: OutputStyle::PLAIN, live: false };
        let mut session = Session::default();
        for (i, step) in STEPS.iter().enumerate() {
            assert!(!step.text.is_empty(), "step {} says nothing", i + 1);
            assert!(!step.examples.is_empty(), "step {} has nothing to type", i + 1);
            let mut last = None;
            for ex in step.examples {
                last = Some((ex, session.enter(ex.source, &opts).output));
            }
            let (ex, out) = last.expect("checked above");
            assert!(
                ex.rejected_with.is_some() || out.body_type.is_some(),
                "step {} ({}) ends on a declaration: {}",
                i + 1,
                step.title,
                ex.source
            );
        }
    }

    /// A line that does not fit an eighty-column terminal (after the two
    /// columns of indent it is shown with) gets wrapped wherever that lands.
    #[test]
    fn nothing_is_too_wide_for_a_narrow_terminal() {
        for step in STEPS {
            for line in step.text {
                assert!(line.chars().count() <= 72, "too wide, and it will wrap: {line:?}");
            }
        }
    }

    /// Each section is one stretch of the tour, in the order of
    /// [`Section::ALL`], which is what makes `:tour grades` mean something.
    #[test]
    fn each_section_is_one_stretch_in_order() {
        let mut seen: Vec<Section> = Vec::new();
        for step in STEPS {
            if seen.last() != Some(&step.section) {
                assert!(!seen.contains(&step.section), "{} is in two places", step.section.title());
                seen.push(step.section);
            }
        }
        assert_eq!(seen, Section::ALL, "every section has steps, in order");
    }

    #[test]
    fn a_section_can_be_asked_for_by_name() {
        assert_eq!(Section::named("grades"), Some(Section::Grades));
        assert_eq!(Section::named("dependent"), Some(Section::Dependent));
        assert_eq!(Section::named("matching"), Some(Section::Matching));
        assert_eq!(Section::named("effects"), Some(Section::Effects));
        assert_eq!(Section::named("3"), None, "numbers are steps");
        assert_eq!(Section::Basics.first(), Some(0));
    }

    #[test]
    fn the_heading_says_where_you_are() {
        assert!(heading(0).starts_with("Step 1 of "));
        assert!(heading(STEPS.len() - 1).contains(&format!("of {}", STEPS.len())));
    }
}
