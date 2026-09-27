//! The compiler pipeline: lex → layout → parse → rename → SCC → type check,
//! and then — for a program that checks — lower to AxCut → linearize → run.
//!
//! Every pass reports plain [`Diagnostic`]s, collected here in pass order.  A
//! parse failure does not short-circuit the report: whatever the lexer and
//! layout already found is still shown.

use idyllic_diagnostics::{self as diagnostics, Diagnostic, OutputStyle};
use idyllic_span::LineIndex;
use idyllic_llvm as llvm;
use {
    idyllic_seq as seq, idyllic_layout as layout, idyllic_lexer as lexer, idyllic_parser as parser,
    idyllic_rename as rename, idyllic_scc as scc, idyllic_typecheck as typecheck,
};

#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Dump the HIR after the SCC pass.
    pub debug: bool,
    pub style: OutputStyle,
}

pub struct Output {
    /// Every diagnostic, in pass order.
    pub diagnostics: Vec<Diagnostic>,
    /// The type of the program's trailing expression, if it has one and
    /// checking got that far.
    pub body_type: Option<String>,
    /// Each top-level definition and its type, in source order.
    pub decls: Vec<(String, String)>,
    /// The HIR dump, when [`Options::debug`] is set.
    pub hir: Option<String>,
    /// The program as AxCut, if it checked and has an expression to run.
    pub axcut: Option<seq::Program>,
    /// The same, inlined: what the native compiler is given.  (What is run
    /// here, and shown, is the program as it was lowered.)
    pub native: Option<seq::Program>,
    /// What running it came to.
    pub evaluation: Option<Evaluation>,
    /// What it did with memory, if it ran to the end.
    pub memory: Option<seq::machine::Memory>,
    /// How compiled code should print the answer.
    pub result_kind: llvm::ResultKind,
    /// The elaborated program, if checking got that far: what is lowered,
    /// and what the reference interpreter (`idyll-eval`) runs.
    pub checked: Option<typecheck::TypeCheckResult>,
}

/// What every program starts with.  Lists are an ordinary data type; what is
/// built in is only the spelling of the constructor, an infix `::`.
pub const PRELUDE: &str = "data List a = Nil | (::) a (List a)";

fn prelude() -> Vec<idyllic_ast::LData> {
    let tokens = lexer::tokenize(PRELUDE).tokens;
    let laid_out = layout::layout(&LineIndex::new(PRELUDE), &tokens);
    parser::parse(PRELUDE.len(), &laid_out.tokens).expect("the prelude parses").datas
}

/// How many steps the AxCut machine gives a program before giving up
/// on it.  Nothing checks that a recursion ends; and one that does, but not
/// soon, is what the native compiler is for.
pub const FUEL: u64 = 1_000_000;

/// What running a program came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Evaluation {
    /// A value that can be shown: a number, a string, a box of one, ...
    Value(String),
    /// A value with nothing to show: a function, a type.
    Opaque,
    /// Still running after [`FUEL`] steps.
    OutOfFuel,
    /// The compiler produced a program it could not run.  That is a bug in
    /// the compiler, never in the program.
    Failed(String),
}

/// Lower, hold the result to its grades, infer the substitutions, and run.
fn evaluate(
    checked: &typecheck::TypeCheckResult,
) -> Option<(seq::Program, Option<seq::Program>, Evaluation, Option<seq::machine::Memory>)> {
    let lowered = match seq::lower::lower(checked)? {
        Ok(lowered) => lowered,
        Err(e) => {
            return Some((
                seq::Program::default(),
                None,
                Evaluation::Failed(e.to_string()),
                None,
            ))
        }
    };
    let program = seq::linearize::linearize(&lowered);
    // a variable used more often than its grade allows: the type checker
    // and the lowering disagree, and one of them is wrong
    if let Some(violation) = seq::grades::check(&lowered).first() {
        return Some((program, None, Evaluation::Failed(violation.to_string()), None));
    }
    let (evaluation, memory) = match seq::machine::run(&program, FUEL) {
        Ok(outcome) if outcome.memory.leaked + outcome.memory.miscounted > 0 => {
            let m = outcome.memory;
            let why = format!("{} blocks leaked and {} miscounted", m.leaked, m.miscounted);
            (Evaluation::Failed(why), Some(m))
        }
        Ok(outcome) => (
            outcome.answer.map_or(Evaluation::Opaque, Evaluation::Value),
            Some(outcome.memory),
        ),
        Err(seq::machine::MachineError::OutOfFuel) => (Evaluation::OutOfFuel, None),
        Err(e) => (Evaluation::Failed(e.to_string()), None),
    };
    // only now: inlining lets an argument be used as its parameter was,
    // which is not what the argument's own grade says
    let native = seq::linearize::linearize(&seq::inline::inline(&lowered));
    Some((program, Some(native), evaluation, memory))
}

impl Output {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }

    /// The program as it was lowered to AxCut, and what running it did with
    /// memory, for `--axcut`; `None` if it never got as far as lowering.
    pub fn axcut_report(&self) -> Option<String> {
        let program = self.axcut.as_ref()?;
        let mut out = format!("{program}\n\n");
        if let Some(memory) = &self.memory {
            out.push_str(&format!("{memory}\n\n"));
        }
        Some(out)
    }

    /// Everything a user should see: rendered diagnostics, then the result —
    /// the value and type of the trailing expression (`3 : Int`; just
    /// `: Int -> Int` when the value is nothing that can be shown), or
    /// failing one, the types of the definitions.
    pub fn display(&self, name: &str, src: &str, style: OutputStyle) -> String {
        let mut out = diagnostics::render(&self.diagnostics, name, src, style);
        if let Some(hir) = &self.hir {
            out.push_str(hir);
            out.push('\n');
        }
        match &self.body_type {
            Some(ty) => {
                if let Some(Evaluation::Value(v)) = &self.evaluation {
                    out.push_str(v);
                    out.push(' ');
                }
                out.push_str(": ");
                out.push_str(ty);
                match &self.evaluation {
                    Some(Evaluation::OutOfFuel) => {
                        out.push_str(&format!("\n(still running after {FUEL} steps; stopped)"));
                    }
                    Some(Evaluation::Failed(why)) => {
                        out.push_str(&format!(
                            "\n(could not be run — this is a bug in idyllic: {why})"
                        ));
                    }
                    _ => {}
                }
            }
            None => {
                let decls: Vec<String> = self
                    .decls
                    .iter()
                    .map(|(x, ty)| format!("{x} : {ty}"))
                    .collect();
                out.push_str(&decls.join("\n"));
            }
        }
        out
    }
}

/// Run the whole pipeline over one source text.  Never fails.
pub fn run(src: &str, opts: &Options) -> Output {
    let li = LineIndex::new(src);
    let mut diagnostics = Vec::new();

    let lexed = lexer::tokenize(src);
    diagnostics.extend(lexed.diagnostics);

    let laid_out = layout::layout(&li, &lexed.tokens);
    diagnostics.extend(
        laid_out
            .errors
            .iter()
            .map(layout::LayoutError::to_diagnostic),
    );

    let ast = match parser::parse(src.len(), &laid_out.tokens) {
        Ok(ast) => ast,
        Err(errs) => {
            diagnostics.extend(errs);
            return Output {
                diagnostics,
                body_type: None,
                decls: Vec::new(),
                hir: None,
                axcut: None,
                native: None,
                evaluation: None,
                memory: None,
                result_kind: llvm::ResultKind::Other,
                checked: None,
            };
        }
    };

    // name resolution -> HIR, then SCC / retopologize, then type check
    let prelude = prelude();
    let renamed = rename::rename_module(&ast, &prelude);
    diagnostics.extend(
        renamed
            .errors
            .iter()
            .map(rename::RenameError::to_diagnostic),
    );
    let module = scc::scc_module(renamed.module);
    let checked = typecheck::check_module(&module, renamed.spans.len(), prelude.len());
    diagnostics.extend(
        checked
            .errors
            .iter()
            .map(|e| e.to_diagnostic(&renamed.spans)),
    );

    // only a program that checks is run: the machine takes the types' word
    // for it that every call has a function and every unboxing a box
    let ran = if diagnostics.iter().any(Diagnostic::is_error) {
        None
    } else {
        evaluate(&checked)
    };
    let (axcut, native, evaluation, memory) = match ran {
        Some((p, n, e, m)) => (Some(p), n, Some(e), m),
        None => (None, None, None, None),
    };

    use idyllic_builtins::Builtin;
    use idyllic_typecheck::core::Core;
    let result_kind = match checked.body_type.as_deref() {
        Some(Core::LitTy(Builtin::Int)) => llvm::ResultKind::Int,
        Some(Core::LitTy(Builtin::Bool)) => llvm::ResultKind::Bool,
        Some(Core::LitTy(Builtin::Unit)) => llvm::ResultKind::Unit,
        Some(Core::LitTy(Builtin::Str)) => llvm::ResultKind::Str,
        _ => llvm::ResultKind::Other,
    };

    Output {
        diagnostics,
        result_kind,
        axcut,
        native,
        evaluation,
        memory,
        body_type: checked.body_type.as_ref().map(|ty| ty.to_string()),
        decls: checked
            .decl_types
            .iter()
            .map(|(x, ty)| (x.clone(), ty.to_string()))
            .collect(),
        hir: opts.debug.then(|| format!("{module:#?}")),
        checked: Some(checked),
    }
}

/// An interactive session: the declarations entered so far stay in scope for
/// later entries.
///
/// They are kept as source, and checked again in front of each new entry.
/// That is slower than keeping the checker's state, but it cannot get out of
/// step with it, and a REPL's worth of declarations checks in no time.
#[derive(Default)]
pub struct Session {
    source: String,
    /// how many definitions `source` makes
    seen: usize,
}

/// The result of one entry.  Spans in `output` index `source`, which is the
/// session's declarations followed by the entry.
pub struct Entered {
    pub output: Output,
    pub source: String,
}

impl Session {
    /// Run one entry.  Only what is new is reported: the earlier entries'
    /// diagnostics and definitions have been shown already.  An entry made
    /// of declarations alone, with no errors, extends the session.
    pub fn enter(&mut self, entry: &str, opts: &Options) -> Entered {
        let source = format!("{}{}\n", self.source, entry.trim_end());
        let mut output = run(&source, opts);
        let earlier = self.source.len();
        output
            .diagnostics
            .retain(|d| d.labels.is_empty() || d.labels.iter().any(|l| l.span.start >= earlier));
        let defined = output.decls.len();
        output.decls.drain(..self.seen.min(defined));
        if output.body_type.is_none() && !output.has_errors() && defined > self.seen {
            self.seen = defined;
            self.source = source.clone();
        }
        Entered { output, source }
    }

    /// Forget every declaration.
    pub fn reset(&mut self) {
        *self = Session::default();
    }
}
