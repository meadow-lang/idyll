//! The rules of how Idyll is set out, and what lays a tree out by them.
//!
//! `rule_of` is front/'s `format!` (`front/src/Syntax.mw`), rule for rule, and
//! the rest is Lingua's `Format.mw`, the engine under `format!`, in the same
//! order and under the same names, so that the two can be read side by side
//! and come out the same: `compiler/idyllic/tests/front.rs` compares them on
//! every program of the corpus.
//!
//! A node with a rule goes on one line if it fits, and breaks where its rule
//! says if not. A node with no rule is written as it was, with its line breaks
//! where they were, set in by what is around it.

use crate::doc::{self, Doc};
use crate::tree::{Branch, Kind, Syntax};

/// How wide a level is.
const UNIT: i64 = 2;

/// How wide a line may be.
const WIDTH: usize = 100;

/// What a rule says a node holds: a token, by its text; a child, by the kinds
/// it may be, one or as many as there are; several of those some columns in,
/// or on one line or broken together; and how they are parted.
#[derive(Clone, Copy)]
enum Elem {
    Tok(&'static str),
    Child(&'static [Kind], bool),
    IndentBy(i64, &'static [Elem]),
    Group(&'static [Elem]),
    Mark(i64),
    /// A rule's first element, for a rule that is not a group of its own.
    NoGroup,
}

use Elem::{Child, Group, IndentBy, Mark, NoGroup, Tok};

// How two things are parted, weakest first: by nothing; by nothing, or a
// break where the group breaks; by a space, or a break where it breaks; by a
// break, always; by a blank line, which is kept where the text had one.
const TIGHT: Elem = Mark(TIGHTLY);
const CUT: Elem = Mark(CUTTING);
const LINE: Elem = Mark(LINING);
const NEWLINE: Elem = Mark(BREAKING);
const TIGHTLY: i64 = 0;
const CUTTING: i64 = 1;
const LINING: i64 = 2;
const BREAKING: i64 = 3;
const BLANKING: i64 = 4;

/// Parts of none: a parting that is not there.
const NONE: i64 = -1;

const TERM: &[Kind] = &[Kind::Lam, Kind::LetE, Kind::CaseE, Kind::IfE, Kind::HandleE, Kind::Op];
const OPENS: Elem = Child(&[Kind::OpenBrace, Kind::OpenLaid], false);
const CLOSES: Elem = Child(&[Kind::CloseBrace, Kind::CloseLaid], false);
const EFFECTS: Elem = Child(&[Kind::Effects], false);
const ATOMS: Elem = Child(
    &[
        Kind::Name,
        Kind::Number,
        Kind::StrLit,
        Kind::CharLit,
        Kind::Hole,
        Kind::GoalA,
        Kind::TypeA,
        Kind::InfA,
        Kind::OpName,
        Kind::Paren,
        Kind::BoxA,
        Kind::SeqA,
        Kind::Braced,
        Kind::RecordLit,
        Kind::DotA,
        Kind::AutoA,
        Kind::InterpA,
    ],
    true,
);
const WHOLE: Elem = Child(TERM, false);
const GROUPED: Elem = Group(&[WHOLE]);
/// A body after its `=` or `->`, or under it a level in.
const BODY: Elem = IndentBy(UNIT, &[LINE, GROUPED]);

/// The rule of a node of this kind.
fn rule_of(kind: Kind) -> Option<&'static [Elem]> {
    Some(match kind {
        // the layout's `;` is a line break; a written one keeps the next item
        // on its line
        Kind::LaidSemi => &[TIGHT, Tok("vsemi"), NEWLINE],
        Kind::WrittenSemi => &[TIGHT, Tok(";")],
        Kind::Plain => &[Tok("="), BODY],
        // guards a line each, a level in
        Kind::Line => &[WHOLE, EFFECTS, Child(&[Kind::Plain, Kind::Guards], false)],
        Kind::Guards => &[IndentBy(UNIT, &[Child(&[Kind::Guard], true)])],
        Kind::Guard => &[NEWLINE, Tok("|"), GROUPED, Tok("="), BODY],
        Kind::AltPlain => &[Tok("->"), BODY],
        Kind::Alt => &[Child(&[Kind::Chain], false), Child(&[Kind::AltPlain, Kind::AltGuards], false)],
        Kind::AltGuards => &[IndentBy(UNIT, &[Child(&[Kind::AltGuard], true)])],
        Kind::AltGuard => &[NEWLINE, Tok("|"), Group(&[Child(&[Kind::Chain], false)]), Tok("->"), BODY],
        Kind::Clause => &[Child(&[Kind::Lower], false), Child(&[Kind::ClauseParam], true), Tok("->"), BODY],
        Kind::Lam => &[Tok("\\"), TIGHT, ATOMS, Tok("->"), BODY],
        Kind::IfE => &[
            Tok("if"),
            GROUPED,
            IndentBy(UNIT, &[LINE, Tok("then"), GROUPED]),
            IndentBy(UNIT, &[LINE, Tok("else"), GROUPED]),
        ],
        // a block's items after what opens it, or under it a level in, each a
        // line of its own; a `let`'s `in` under the `let`. A block of
        // declarations always under it.
        Kind::LetE => &[
            Tok("let"),
            OPENS,
            IndentBy(UNIT, &[LINE, Child(&[Kind::Line, Kind::WrittenSemi, Kind::LaidSemi], true)]),
            CLOSES,
            LINE,
            Tok("in"),
            GROUPED,
        ],
        Kind::CaseE => &[
            Tok("case"),
            GROUPED,
            Tok("of"),
            OPENS,
            IndentBy(UNIT, &[LINE, Child(&[Kind::Alt, Kind::WrittenSemi, Kind::LaidSemi], true)]),
            CLOSES,
        ],
        Kind::HandleE => &[
            Tok("handle"),
            GROUPED,
            Tok("with"),
            OPENS,
            IndentBy(UNIT, &[LINE, Child(&[Kind::Clause, Kind::WrittenSemi, Kind::LaidSemi], true)]),
            CLOSES,
        ],
        Kind::CtorSigs => &[
            Tok("where"),
            OPENS,
            IndentBy(UNIT, &[NEWLINE, Child(&[Kind::CtorSig, Kind::WrittenSemi, Kind::LaidSemi], true)]),
            CLOSES,
        ],
        Kind::TraitDecl => &[Tok("trait"), GROUPED, Tok("where"), OPENS, DECLARATIONS, CLOSES],
        Kind::InstanceDecl => &[
            Child(&[Kind::Instance, Kind::InstanceNamed], false),
            Child(&[Kind::InstanceName], false),
            GROUPED,
            Tok("where"),
            OPENS,
            DECLARATIONS,
            CLOSES,
        ],
        Kind::OpBlock => &[
            Tok("where"),
            OPENS,
            IndentBy(UNIT, &[NEWLINE, Child(&[Kind::OpSig, Kind::WrittenSemi, Kind::LaidSemi], true)]),
            CLOSES,
        ],
        Kind::ModuleDecl => &[
            Tok("module"),
            Child(&[Kind::Upper], false),
            Tok("where"),
            OPENS,
            IndentBy(UNIT, &[NEWLINE, Child(&[Kind::Item, Kind::WrittenSemi, Kind::LaidSemi], true)]),
            CLOSES,
        ],
        // a signature that does not fit an entry a line: the name alone, the
        // colon three spaces in and each arrow two, so every type starts in
        // one column. A chain of arrows breaks as one (`NoGroup`: an arrow's
        // line breaks with the group around it).
        Kind::Annot => &[IndentBy(2, &[LINE, IndentBy(1, &[Tok(":")]), WHOLE])],
        // elsewhere a type is a group of its own, so that its arrows break
        // only if it does not fit
        Kind::OpSig => &[Child(&[Kind::OpAttr], false), Child(&[Kind::Lower], false), Tok(":"), GROUPED],
        Kind::CtorSig => &[Child(&[Kind::PlainCtor, Kind::ParenCtor], false), Tok(":"), GROUPED],
        Kind::DataKind => &[Tok(":"), GROUPED],
        Kind::FieldDecl => &[Child(&[Kind::Lower], false), Tok(":"), GROUPED],
        // an attribute is a line of its own
        Kind::AttrLine => &[
            Tok("@"),
            TIGHT,
            Child(&[Kind::Lower], false),
            TIGHT,
            Child(&[Kind::AttrArgs], false),
            TIGHT,
            Child(&[Kind::Semi, Kind::LaidSep], false),
            NEWLINE,
        ],
        // a sequence on one line, or an item a line under what opens it
        Kind::SeqA => &[Tok("#"), TIGHT, Tok("["), IndentBy(UNIT, &[CUT, WHOLE, Child(&[Kind::SeqRest], true)]), CUT, Tok("]")],
        Kind::SeqRest => &[NoGroup, TIGHT, Child(&[Kind::Comma, Kind::Semi], false), LINE, WHOLE],
        Kind::Op => &[
            NoGroup,
            Child(&[Kind::Chain], false),
            Child(&[Kind::Annot, Kind::Arrow, Kind::Constraint], false),
        ],
        Kind::Arrow => &[NoGroup, LINE, Tok("->"), WHOLE, EFFECTS],
        Kind::Constraint => &[NoGroup, LINE, Tok("=>"), WHOLE, EFFECTS],
        _ => return None,
    })
}

const DECLARATIONS: Elem = IndentBy(UNIT, &[NEWLINE, Child(&[Kind::Line, Kind::WrittenSemi, Kind::LaidSemi], true)]);

/// A piece of a node being laid out: a parting -- how strong, and how many
/// columns in -- or something to write, at a column, and the partings it
/// begins and ends with of its own: a break it begins or ends with stands with
/// one beside it rather than as well as it.
#[derive(Clone)]
enum Item {
    Parting(i64, i64),
    Written(i64, Doc, i64, i64),
}

use Item::{Parting, Written};

/// How many breaks a parting is: none, one, or -- a blank line -- two.
fn breaks_of(strength: i64) -> i64 {
    if strength >= BLANKING {
        2
    } else if strength >= BREAKING {
        1
    } else {
        0
    }
}

/// `doc` and `n` breaks after it.
fn breaks_after(doc: Doc, n: i64) -> Doc {
    (0..n).fold(doc, |d, _| doc::append(d, doc::hardline()))
}

/// The tree's text laid out: each node with a rule on one line if it fits,
/// and broken where its rule says if not.
pub fn layout(src: &str, tree: &Syntax) -> String {
    let st = Style { src };
    let out = doc::render(WIDTH, st.doc_of(tree, true));
    // A line of nothing but its indent is an empty one; and the text ends
    // with a line's end if it did.
    let lines: Vec<&str> = out.split('\n').map(|l| if l.trim().is_empty() { "" } else { l }).collect();
    let mut text = lines.join("\n").trim_end().to_string();
    if src.ends_with('\n') && !out.trim().is_empty() {
        text.push('\n');
    }
    text
}

/// What a node's parts are as what is around it sees it: the parting its rule
/// begins with; what it writes, with what that begins and ends with of its
/// own; and the parting its rule ends with.
type Parts = (i64, Option<(Doc, i64, i64)>, i64);

/// A node no rule is for being written: what is written so far; the breaks
/// owed, and whether a rule asked for them; how many it ends with already;
/// the space the text had, which a break that follows drops; how far in the
/// line the text broke to was written; how far in the line being written is
/// set; and whether anything is written yet, before which blank space is no
/// one's.
struct Writing {
    doc: Doc,
    owed: i64,
    asked: bool,
    broke: i64,
    gap: String,
    col: i64,
    cur: i64,
    shown: bool,
}

struct Style<'s> {
    src: &'s str,
}

impl<'s> Style<'s> {
    fn doc_of(&self, s: &Syntax, root: bool) -> Doc {
        match s {
            Syntax::Leaf(_) => doc::text(s.text(self.src)),
            Syntax::Branch(b) => match rule_of(b.kind) {
                Some(elems) => self.ruled_doc(b, elems),
                None => {
                    let (d, n, _) = self.written_parts(b, root);
                    breaks_after(d, n)
                }
            },
        }
    }

    /// How far in the line of the text that `at` is on starts.
    fn indent_of_line(&self, at: usize) -> i64 {
        let bytes = self.src.as_bytes();
        let at = at.min(bytes.len());
        let start = bytes[..at].iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        bytes[start..].iter().take_while(|&&b| b == b' ' || b == b'\t').count() as i64
    }

    /// `d` written next, which begins with `opens` breaks of its own. A line
    /// a rule broke to -- the layout's `;` before it, say -- or one that
    /// begins with a break of its own is set in as what is around it says. A
    /// line only the text broke to goes on with the one before: it is as far
    /// in from the node's first line as it was, and further in than that line
    /// by a level at least -- at its column, in a language whose layout is
    /// part of its meaning, it would be a new item.
    fn put(&self, w: &mut Writing, base: i64, opens: i64, d: Doc) {
        let n = (w.owed - w.broke - opens).max(0);
        let fresh = w.owed > 0 || w.broke > 0 || opens > 0;
        let set = if !fresh {
            w.cur
        } else if opens > 0 || w.asked || w.col < 0 {
            0
        } else {
            (w.col - base).max(UNIT)
        };
        let lead = if fresh {
            breaks_after(doc::nil(), n)
        } else {
            doc::text(if w.shown { &w.gap } else { "" })
        };
        w.doc = doc::append(w.doc.clone(), doc::nest(set, doc::append(lead, d)));
        w.owed = 0;
        w.asked = false;
        w.broke = 0;
        w.gap = String::new();
        w.col = -1;
        w.cur = set;
        w.shown = true;
    }

    /// A node no rule is for, as it was written: its tokens and children in
    /// turn, a break where there was one -- and a blank line where there was
    /// -- and the spaces between as they were; but for the breaks it ends
    /// with: how many, for what holds it to write as a parting, and whether a
    /// rule asked for them. A line the text broke to is as far in from the
    /// line the node began on as it was, but for a child whose rule begins
    /// with a break, which its rule sets in.
    fn written_parts(&self, b: &Branch, root: bool) -> (Doc, i64, bool) {
        let all = &b.kids;
        let base = self.indent_of_line(b.from);
        let blank = |c: &Syntax| c.is_trivia() && c.text(self.src).trim().is_empty();
        let (Some(first), Some(last)) = (all.iter().position(|c| !blank(c)), all.iter().rposition(|c| !blank(c))) else {
            return (doc::nil(), 0, false);
        };
        let mut w = Writing {
            doc: doc::nil(),
            owed: 0,
            asked: false,
            broke: 0,
            gap: String::new(),
            col: -1,
            cur: 0,
            shown: false,
        };
        for c in &all[first..=last] {
            match c {
                _ if c.is_trivia() => {
                    let t = c.text(self.src);
                    if !t.trim().is_empty() {
                        // A comment on a line of its own between two items
                        // is neither's: the next line is set in as it would be.
                        let asked = w.asked;
                        self.put(&mut w, base, 0, doc::text(t.trim()));
                        w.asked = asked;
                    } else {
                        let lines: Vec<&str> = t.split('\n').collect();
                        let n = 2.min(lines.len() as i64 - 1);
                        if n == 0 {
                            if w.owed <= 0 && w.broke <= 0 {
                                w.gap = t.to_string();
                            }
                        } else {
                            w.owed = w.owed.max(n);
                            w.gap = String::new();
                            w.col = lines[lines.len() - 1].len() as i64;
                        }
                    }
                }
                Syntax::Leaf(_) => {
                    let t = c.text(self.src);
                    if !t.is_empty() {
                        self.put(&mut w, base, 0, doc::text(t));
                    }
                }
                Syntax::Branch(child) => match rule_of(child.kind) {
                    None => {
                        let (d, n, a) = self.written_parts(child, false);
                        self.put(&mut w, base, 0, d);
                        w.owed = n;
                        w.asked = a;
                    }
                    Some(elems) => {
                        let (lead, body, trail) = self.ruled_parts(child, elems);
                        if breaks_of(lead) > 0 {
                            w.owed = w.owed.max(breaks_of(lead));
                            w.asked = true;
                        }
                        match body {
                            Some((d, opens, closes)) => {
                                self.put(&mut w, base, breaks_of(opens), d);
                                w.owed = breaks_of(trail);
                                w.asked = breaks_of(trail) > 0 || breaks_of(closes) > 0;
                                w.broke = breaks_of(closes);
                            }
                            None => {
                                if breaks_of(trail) > 0 {
                                    w.owed = w.owed.max(breaks_of(trail));
                                    w.asked = true;
                                }
                            }
                        }
                    }
                },
            }
        }
        let n = (w.owed - w.broke).max(0);
        // A comment that ends it is followed by a break, so that nothing
        // after the node is taken for part of the comment -- but not at the
        // end of the text.
        if all[last].is_trivia() && !root {
            (w.doc, n.max(1), w.asked)
        } else {
            (w.doc, n, w.asked)
        }
    }

    /// A rule's items as one document: a group, unless the rule says `NoGroup`.
    fn as_rule(&self, elems: &[Elem], items: &[Item]) -> Doc {
        if elems.iter().any(|e| matches!(e, NoGroup)) {
            assemble(items)
        } else {
            doc::group(assemble(items))
        }
    }

    fn ruled_doc(&self, b: &Branch, elems: &[Elem]) -> Doc {
        self.as_rule(elems, &self.ruled_items(b, elems))
    }

    /// A node as its rule lays it out, as what is around it sees it: the
    /// parting its rule begins with -- the strongest, if several -- what it
    /// writes, and the parting its rule ends with. A parting is `NONE` where
    /// there is none, and a node with nothing to write -- a layout `;` -- is
    /// its partings.
    fn ruled_parts(&self, b: &Branch, elems: &[Elem]) -> Parts {
        let items = self.ruled_items(b, elems);
        let leading = items.iter().take_while(|i| matches!(i, Parting(_, 0))).count();
        let middle = &items[leading..];
        // What it ends with, at any level, parts it from what follows -- which
        // a break is set in as, whatever level it was written at.
        let trailing = middle.iter().rev().take_while(|i| matches!(i, Parting(..))).count();
        let inner = &middle[..middle.len() - trailing];
        let strongest = |is: &[Item]| {
            is.iter().fold(NONE, |m, i| match i {
                Parting(n, _) => m.max(*n),
                Written(..) => m.max(NONE),
            })
        };
        (
            strongest(&items[..leading]),
            (!inner.is_empty()).then(|| (self.as_rule(elems, inner), opening_of(inner), closing_of(inner))),
            strongest(&middle[middle.len() - trailing..]),
        )
    }

    fn ruled_items(&self, b: &Branch, elems: &[Elem]) -> Vec<Item> {
        let all = &b.kids;
        // Where each child that is not trivia is among them all.
        let places: Vec<usize> = (0..all.len()).filter(|&i| !all[i].is_trivia()).collect();
        let lv = leaves(elems);
        let found = self.match_leaves(all, &places, &lv);
        let ctx = Holding { all, places: &places, found: &found };
        let (mut items, _, taken) = self.items_of(&ctx, elems, 0, 0, -1);
        for j in (taken + 1)..places.len() as i64 {
            items.extend(self.kid_item(&ctx, 0, j as usize));
        }
        let last = places.last().map_or(0, |&p| p + 1);
        // Comments after a node's last child: each written, and a break after
        // it, so that nothing which follows is taken for part of it.
        for c in &all[last..] {
            let t = c.text(self.src).trim();
            if !t.is_empty() {
                items.push(Written(0, doc::text(t), NONE, NONE));
                items.push(Parting(BREAKING, 0));
            }
        }
        items
    }

    /// Each leaf against the children, from the first on: which of them it is
    /// -- the first and the last, or `None`.
    fn match_leaves(&self, all: &[Syntax], places: &[usize], leaves: &[Elem]) -> Vec<Option<(usize, usize)>> {
        let kid = |j: usize| places.get(j).map(|&p| &all[p]);
        let fits = |kinds: &[Kind], j: usize| kid(j).is_some_and(|c| kinds.contains(&c.kind()));
        let mut at = 0;
        leaves
            .iter()
            .map(|e| match e {
                Tok(t) => {
                    let found = (at..places.len())
                        .find(|&j| kid(j).is_some_and(|c| matches!(c, Syntax::Leaf(_)) && c.text(self.src) == *t))?;
                    at = found + 1;
                    Some((found, found))
                }
                Child(kinds, many) => {
                    if !fits(kinds, at) {
                        return None;
                    }
                    let first = at;
                    let mut last = at;
                    while *many && fits(kinds, last + 1) {
                        last += 1;
                    }
                    at = last + 1;
                    Some((first, last))
                }
                _ => None,
            })
            .collect()
    }

    /// The items of `es`, `level` columns in: `leaf` is how many leaves came
    /// before, and `taken` the last child written. A child the rule did not
    /// name is written before the next one it did, so nothing that was in the
    /// text is left out. Between two of the nodes a child that is many names,
    /// a blank line in the text is kept.
    fn items_of(&self, ctx: &Holding, es: &[Elem], level: i64, leaf: usize, taken: i64) -> (Vec<Item>, usize, i64) {
        let mut out = Vec::new();
        let (mut at, mut done) = (leaf, taken);
        for e in es {
            match e {
                Mark(strength) => out.push(Parting(*strength, level)),
                NoGroup => {}
                // nothing at all where it has nothing to write -- an empty
                // list -- not the partings around that nothing
                IndentBy(n, inner) => {
                    let (items, at2, done2) = self.items_of(ctx, inner, level + n, at, done);
                    if !items.iter().all(|i| matches!(i, Parting(..))) {
                        out.extend(items);
                    }
                    (at, done) = (at2, done2);
                }
                Group(inner) => {
                    let (items, at2, done2) = self.items_of(ctx, inner, 0, at, done);
                    (at, done) = (at2, done2);
                    // What it ends with parts it from what follows.
                    let tail = items.iter().rev().take_while(|i| matches!(i, Parting(..))).count();
                    let (body, after) = items.split_at(items.len() - tail);
                    if !body.is_empty() {
                        out.push(Written(level, doc::group(assemble(body)), opening_of(body), NONE));
                    }
                    out.extend(after.iter().map(|i| match i {
                        Parting(n, _) => Parting(*n, level),
                        other => other.clone(),
                    }));
                }
                _ => {
                    let found = ctx.found[at];
                    at += 1;
                    let Some((first, last)) = found else { continue };
                    for j in (done + 1)..first as i64 {
                        out.extend(self.kid_item(ctx, level, j as usize));
                    }
                    for j in first..=last {
                        if j > first && self.blank_before(ctx, j) {
                            out.push(Parting(BLANKING, level));
                        }
                        out.extend(self.kid_item(ctx, level, j));
                    }
                    done = last as i64;
                }
            }
        }
        (out, at, done)
    }

    /// Whether the text between child `j` and the one before it has a blank line.
    fn blank_before(&self, ctx: &Holding, j: usize) -> bool {
        let from = ctx.places[j - 1] + 1;
        let text: String = ctx.all[from..ctx.places[j]].iter().map(|c| c.text(self.src)).collect();
        let pieces: Vec<&str> = text.split('\n').collect();
        pieces.len() > 2 && pieces[1..pieces.len() - 1].iter().any(|p| p.trim().is_empty())
    }

    /// Child `j`, with the comments written before it, each on a line of its
    /// own: the partings its rule begins and ends with, standing with those
    /// around it, and what it writes. A token with no text is nothing -- but
    /// the comments. A comment on the line of what is before it stays at that
    /// line's end, and a break follows it. Nothing written ends with a break:
    /// a break written at the end of something is set in as what follows it
    /// is, not as itself, so a break after it is a parting, which what follows
    /// sets in.
    fn kid_item(&self, ctx: &Holding, level: i64, j: usize) -> Vec<Item> {
        let all = ctx.all;
        let at = ctx.places[j];
        let before = if j == 0 { 0 } else { ctx.places[j - 1] + 1 };
        let found: Vec<(usize, &str)> = (before..at)
            .filter_map(|i| {
                let t = all[i].text(self.src).trim();
                (!t.is_empty()).then_some((i, t))
            })
            .collect();
        let ends_line = found.first().is_some_and(|&(i, _)| j > 0 && !(before..i).any(|k| all[k].text(self.src).contains('\n')));
        let mut items = Vec::new();
        if let (true, Some(&(_, t))) = (ends_line, found.first()) {
            items.push(Written(level, doc::text(t), NONE, NONE));
            items.push(Parting(BREAKING, level));
        }
        let comments: Vec<&str> = found.iter().skip(ends_line as usize).map(|&(_, t)| t).collect();
        let commented = |own: Doc| {
            comments.iter().rev().fold(own, |d, c| doc::append(doc::append(doc::text(c), doc::hardline()), d))
        };
        let only_comments = || -> Vec<Item> {
            if comments.is_empty() {
                return Vec::new();
            }
            let joined = comments.iter().skip(1).fold(doc::text(comments[0]), |acc, c| {
                doc::append(doc::append(acc, doc::hardline()), doc::text(c))
            });
            vec![Written(level, joined, NONE, NONE), Parting(BREAKING, level)]
        };
        match &all[at] {
            c @ Syntax::Leaf(_) => {
                let t = c.text(self.src);
                if t.is_empty() {
                    items.extend(only_comments());
                } else {
                    items.push(Written(level, commented(doc::text(t)), NONE, NONE));
                }
            }
            Syntax::Branch(c) => match rule_of(c.kind) {
                None => {
                    let (d, n, _) = self.written_parts(c, false);
                    items.push(Written(level, commented(d), NONE, NONE));
                    if n > 0 {
                        items.push(Parting(if n > 1 { BLANKING } else { BREAKING }, level));
                    }
                }
                Some(elems) => {
                    let (lead, body, trail) = self.ruled_parts(c, elems);
                    if lead >= 0 {
                        items.push(Parting(lead, level));
                    }
                    match body {
                        Some((d, opens, closes)) => {
                            let opening = if comments.is_empty() { opens } else { NONE };
                            items.push(Written(level, commented(d), opening, closes));
                        }
                        None => items.extend(only_comments()),
                    }
                    if trail >= 0 {
                        items.push(Parting(trail, level));
                    }
                }
            },
        }
        items
    }
}

/// A node's children, as the rule being read goes over them.
struct Holding<'t> {
    all: &'t [Syntax],
    places: &'t [usize],
    found: &'t [Option<(usize, usize)>],
}

/// The tokens and children a rule names, in order, through what encloses them.
fn leaves(es: &[Elem]) -> Vec<Elem> {
    es.iter()
        .flat_map(|e| match e {
            IndentBy(_, inner) | Group(inner) => leaves(inner),
            Mark(_) | NoGroup => Vec::new(),
            other => vec![*other],
        })
        .collect()
}

/// The parting items begin with, of their own -- or `NONE`.
fn opening_of(items: &[Item]) -> i64 {
    match items.first() {
        Some(Parting(n, _)) => *n,
        Some(Written(_, _, o, _)) => *o,
        None => NONE,
    }
}

/// The parting items end with, of their own -- or `NONE`.
fn closing_of(items: &[Item]) -> i64 {
    match items.last() {
        Some(Parting(n, _)) => *n,
        Some(Written(_, _, _, c)) => *c,
        None => NONE,
    }
}

/// Items as one document: each at its column, a parting as what it is, and a
/// space between two written things nothing parts. Of several partings in a
/// row the strongest stands -- and of a parting and a break beside it that
/// something written begins or ends with of its own, what the stronger asks.
fn assemble(items: &[Item]) -> Doc {
    let (mut out, mut pending, mut level, mut closed) = (doc::nil(), NONE, 0, TIGHTLY);
    for item in items {
        match item {
            Parting(strength, at) => {
                pending = pending.max(*strength);
                level = *at;
            }
            Written(at, d, opens, closes) => {
                // What ended the last written thing is there only if it is a
                // break: a softer parting at the end of a group writes nothing.
                let beside = (*opens).max(if closed >= BREAKING { closed } else { NONE });
                // A break is set in as what follows it is, so the two are
                // nested together.
                let sep = if pending >= 0 {
                    besides(pending, beside)
                } else if closed >= 0 || *opens >= 0 {
                    doc::nil()
                } else {
                    doc::text(" ")
                };
                out = doc::append(out, doc::nest(*at, doc::append(sep, d.clone())));
                pending = NONE;
                level = 0;
                closed = *closes;
            }
        }
    }
    // A parting with nothing after it parts nothing -- unless it is a break
    // that was asked for.
    if pending >= BREAKING && closed < BREAKING {
        doc::append(out, doc::nest(level, doc::hardline()))
    } else {
        out
    }
}

/// A parting, where what is beside it begins or ends with one of its own,
/// `beside`: only what the parting asks for that that does not.
fn besides(pending: i64, beside: i64) -> Doc {
    if beside < 0 {
        parting_doc(pending)
    } else if pending <= beside {
        doc::nil()
    } else if pending >= BLANKING && beside >= BREAKING {
        doc::hardline()
    } else {
        parting_doc(pending)
    }
}

fn parting_doc(strength: i64) -> Doc {
    match strength {
        TIGHTLY => doc::nil(),
        CUTTING => doc::line_(),
        LINING => doc::line(),
        BREAKING => doc::hardline(),
        _ => doc::append(doc::hardline(), doc::hardline()),
    }
}
