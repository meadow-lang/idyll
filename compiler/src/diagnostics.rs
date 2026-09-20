//! One diagnostic type for every pass, rendered through [ariadne].
//!
//! Each pass produces plain [`Diagnostic`] values — a severity, a headline, a
//! few labelled spans and optional help — and never touches ariadne itself.
//! [`render`] turns the whole batch into text once, against the source, so all
//! passes look the same and the terminal-capability decisions live in exactly
//! one place ([`OutputStyle`]).

use crate::span::Span;
use ariadne::{sources, CharSet, Color, Config, IndexType, Label as ALabel, Report, ReportKind};
use std::io::IsTerminal;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub span: Span,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    /// The first label is the primary one and decides where the report points.
    pub labels: Vec<Label>,
    pub help: Vec<String>,
}

impl Diagnostic {
    pub fn error(message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Error,
            message: message.into(),
            labels: Vec::new(),
            help: Vec::new(),
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Diagnostic {
            severity: Severity::Warning,
            ..Diagnostic::error(message)
        }
    }

    pub fn label(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label { span, message: message.into() });
        self
    }

    pub fn help(mut self, message: impl Into<String>) -> Self {
        self.help.push(message.into());
        self
    }

    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    pub fn primary_span(&self) -> Span {
        self.labels.first().map_or(Span::at(0), |l| l.span)
    }
}

/// What the terminal we are printing to can render.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputStyle {
    /// Box-drawing gutters rather than ASCII art.
    pub unicode: bool,
    /// ANSI colour.
    pub color: bool,
}

impl OutputStyle {
    pub const PLAIN: OutputStyle = OutputStyle { unicode: false, color: false };

    /// Colour follows the usual conventions — on for a terminal, off for a
    /// pipe or `TERM=dumb`, with `NO_COLOR` forcing it off and `FORCE_COLOR`
    /// / `CLICOLOR_FORCE` forcing it on.  Unicode is on unless the terminal
    /// declares itself dumb; the `--ascii` flag overrides it.
    pub fn detect() -> Self {
        let set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
        let dumb = std::env::var("TERM").is_ok_and(|t| t == "dumb");
        let tty = std::io::stdout().is_terminal();
        let color = if set("NO_COLOR") {
            false
        } else if set("FORCE_COLOR") || set("CLICOLOR_FORCE") {
            true
        } else {
            tty && !dumb
        };
        OutputStyle { unicode: !dumb, color }
    }
}

/// Render diagnostics in order, quoting the offending source lines.
pub fn render(diags: &[Diagnostic], file: &str, src: &str, style: OutputStyle) -> String {
    let mut buf: Vec<u8> = Vec::new();
    let config = || {
        Config::new()
            .with_index_type(IndexType::Byte)
            .with_char_set(if style.unicode { CharSet::Unicode } else { CharSet::Ascii })
            .with_color(style.color)
    };
    for d in diags {
        let (kind, primary) = match d.severity {
            Severity::Error => (ReportKind::Error, Color::Red),
            Severity::Warning => (ReportKind::Warning, Color::Yellow),
        };
        let mut report = Report::build(kind, (file.to_string(), d.primary_span().range()))
            .with_config(config())
            .with_message(&d.message);
        for (i, l) in d.labels.iter().enumerate() {
            let color = if i == 0 { primary } else { Color::Blue };
            report = report.with_label(
                ALabel::new((file.to_string(), l.span.range()))
                    .with_message(&l.message)
                    .with_color(color),
            );
        }
        for h in &d.help {
            report = report.with_help(h);
        }
        report
            .finish()
            .write(sources([(file.to_string(), src)]), &mut buf)
            .expect("writing to an in-memory buffer cannot fail");
    }
    String::from_utf8(buf).expect("ariadne emits UTF-8")
}
