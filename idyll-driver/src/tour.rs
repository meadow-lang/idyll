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
    /// Linearity: how many times things are used, and saying so.
    Grades,
    /// Types as values: universes, polymorphism, implicit arguments.
    Dependent,
}

impl Section {
    pub const ALL: [Section; 3] = [Section::Basics, Section::Grades, Section::Dependent];

    pub fn title(self) -> &'static str {
        match self {
            Section::Basics => "the basics",
            Section::Grades => "linearity and grades",
            Section::Dependent => "dependent types",
        }
    }

    /// What `:tour <word>` answers to.  (Numbers are steps, not sections.)
    pub fn named(word: &str) -> Option<Section> {
        match word {
            "basics" | "basic" => Some(Section::Basics),
            "grades" | "grade" | "linearity" => Some(Section::Grades),
            "dependent" | "types" => Some(Section::Dependent),
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
        ],
        examples: &[
            ok("data Op = Add | Sub"),
            ok("apply : Op -> Int -> Int -> Int\napply op x y = case op of\n  Add -> x + y\n  Sub -> x - y"),
            ok("apply Sub 10 3"),
            ok("if apply Add 1 1 == 2 then \"yes\" else \"no\""),
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
    // -----------------------------------------------------------------------
    // Linearity and grades
    // -----------------------------------------------------------------------
    Step {
        title: "Linear by default",
        section: Section::Grades,
        text: &[
            "Functions are linear by default: `Int -> Int` promises to use",
            "its argument exactly once. A value can't be silently dropped or",
            "copied, which is what lets a type describe a resource, such as",
            "memory that must be freed, and not just data.",
            "",
            "So using `x` twice is an error. A grade, in brackets before the",
            "type, says how many uses to expect instead.",
        ],
        examples: &[
            rejected("\\(x : Int) -> x + x", "usage mismatch"),
            ok("\\(x : [2] Int) -> x + x"),
            ok("\\(x : [0] Int) -> 5"),
        ],
    },
    Step {
        title: "Ranges",
        section: Section::Grades,
        text: &[
            "A grade can be a range. `0..1` is \"at most once\", `1..inf` is",
            "\"at least once\", and empty brackets mean any number of times.",
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
            ok("data Maybe a = Nothing | Just a"),
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
            "works for a function of any grade, handed a box to match.",
        ],
        examples: &[
            ok("app : {a b : Type} {r : Grade} (f : a [r] -> b) -> a [r] -> b\napp f [x] = f [x]"),
            ok("app copy [4]"),
            rejected("\\(y : Int) -> app copy [y]", "usage mismatch"),
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
            "every arrow. With `MkBad (Bad -> Int)` you could apply what is",
            "inside a `Bad` to the `Bad` itself, and loop with no recursion",
            "anywhere. Inside another positive type, such as `List`, is fine.",
        ],
        examples: &[
            rejected("data U = MkU Type", "field too large for its data type"),
            rejected("data Bad = MkBad (Bad -> Int)", "Bad is not strictly positive"),
            ok("data Rose a = Node a (List (Rose a))"),
            ok("Node 1 (Node 2 Nil :: Nil)"),
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
        let opts = Options { debug: false, style: OutputStyle::PLAIN };
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
        let opts = Options { debug: false, style: OutputStyle::PLAIN };
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
        let opts = Options { debug: false, style: OutputStyle::PLAIN };
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
        assert_eq!(Section::named("3"), None, "numbers are steps");
        assert_eq!(Section::Basics.first(), Some(0));
    }

    #[test]
    fn the_heading_says_where_you_are() {
        assert!(heading(0).starts_with("Step 1 of "));
        assert!(heading(STEPS.len() - 1).contains(&format!("of {}", STEPS.len())));
    }
}
