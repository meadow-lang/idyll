//! Source positions.  Everything downstream of the lexer talks about source
//! locations as [`Span`]s: absolute, 0-based, half-open byte offset ranges into
//! the original text.  Line and column numbers are only computed at the edges
//! (the layout pass and diagnostics), via [`LineIndex`].

use std::ops::Range;

/// A half-open byte range `start..end` into the source text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }

    /// The zero-width span at an offset.
    pub const fn at(offset: usize) -> Self {
        Span { start: offset, end: offset }
    }

    /// The smallest span covering both.
    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    pub fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    pub fn range(self) -> Range<usize> {
        self.start..self.end
    }
}

impl From<Range<usize>> for Span {
    fn from(r: Range<usize>) -> Self {
        Span::new(r.start, r.end)
    }
}

impl From<Span> for Range<usize> {
    fn from(s: Span) -> Self {
        s.range()
    }
}

impl From<chumsky::span::SimpleSpan> for Span {
    fn from(s: chumsky::span::SimpleSpan) -> Self {
        Span::new(s.start, s.end)
    }
}

/// A value together with the span it came from.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Spanned { node, span }
    }
}

/// Byte offset → (line, column) conversion, both 1-based.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineIndex {
    /// Byte offset at which each line begins.  Always starts with 0.
    line_starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(src: &str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(
            src.bytes()
                .enumerate()
                .filter(|&(_, b)| b == b'\n')
                .map(|(i, _)| i + 1),
        );
        LineIndex { line_starts }
    }

    /// 1-based (line, column) of a byte offset.  Columns count bytes from the
    /// start of the line, which is exact for ASCII and monotone otherwise.
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        // index of the last line start <= offset
        let line = self
            .line_starts
            .partition_point(|&s| s <= offset)
            .saturating_sub(1);
        let col = offset - self.line_starts[line] + 1;
        (line + 1, col)
    }

    pub fn column(&self, offset: usize) -> usize {
        self.line_col(offset).1
    }
}
