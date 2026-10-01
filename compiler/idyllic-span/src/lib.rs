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

    /// The same span, around something made from what was there.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Spanned<U> {
        Spanned { node: f(self.node), span: self.span }
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

/// Which file of a [`SourceMap`] something is in.
pub type FileId = usize;

/// A file of source, as a [`SourceMap`] holds it: its text starts at offset
/// `base` of the one address space all the map's spans share.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceFile {
    pub name: String,
    pub text: String,
    pub base: usize,
}

impl SourceFile {
    /// Where the file's text ends, in the shared address space.
    pub fn end(&self) -> usize {
        self.base + self.text.len()
    }
}

/// Every file of a program, in one address space: each file's offsets come
/// after the last's, so a [`Span`] says which file it is in as well as
/// where, and nothing downstream of the lexer needs to know there is more
/// than one file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    /// Add a file, after the others (with a byte between, so that the end of
    /// one is never the start of the next).
    pub fn add(&mut self, name: impl Into<String>, text: impl Into<String>) -> FileId {
        let base = self.files.last().map_or(0, |f| f.end() + 1);
        self.files.push(SourceFile { name: name.into(), text: text.into(), base });
        self.files.len() - 1
    }

    pub fn file(&self, id: FileId) -> &SourceFile {
        &self.files[id]
    }

    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }

    /// The file an offset is in, and the offset within it.
    pub fn locate(&self, offset: usize) -> Option<(FileId, usize)> {
        let i = self.files.partition_point(|f| f.base <= offset).checked_sub(1)?;
        let f = &self.files[i];
        (offset <= f.end()).then(|| (i, offset - f.base))
    }
}

impl SourceMap {
    /// Call a file something else, for showing it.
    pub fn set_name(&mut self, id: FileId, name: impl Into<String>) {
        self.files[id].name = name.into();
    }
}
