//! Documents and laying them out: Wadler's printer, as the Pretty library
//! front/ uses has it (itself a port of the `pretty` crate), so that a line
//! breaks here where it breaks there.
//!
//! The one thing to know is that a break is set in as what *follows* it is:
//! a `Hardline` takes the next command off the stack and starts a line at
//! that command's indentation. So a break and what it leads to are nested
//! together.

use std::rc::Rc;
use unicode_width::UnicodeWidthStr;

pub type Doc = Rc<Node>;

pub enum Node {
    Empty,
    Append(Doc, Doc),
    Group(Doc),
    /// As it is broken, and as it is flat.
    FlatAlt(Doc, Doc),
    Nest(i64, Doc),
    Hardline,
    /// The text, and how many columns it takes.
    Text(String, usize),
}

pub fn nil() -> Doc {
    Rc::new(Node::Empty)
}

pub fn hardline() -> Doc {
    Rc::new(Node::Hardline)
}

fn space() -> Doc {
    text(" ")
}

/// A line break, or a space where the group fits on one line.
pub fn line() -> Doc {
    Rc::new(Node::FlatAlt(hardline(), space()))
}

/// A line break, or nothing where the group fits on one line.
pub fn line_() -> Doc {
    Rc::new(Node::FlatAlt(hardline(), nil()))
}

/// `s`, measured in bytes if it is all ASCII and in terminal columns if not.
pub fn text(s: &str) -> Doc {
    if s.is_empty() {
        return nil();
    }
    let width = if s.is_ascii() { s.len() } else { s.width() };
    Rc::new(Node::Text(s.to_string(), width))
}

pub fn append(a: Doc, b: Doc) -> Doc {
    match (&*a, &*b) {
        (Node::Empty, _) => b,
        (_, Node::Empty) => a,
        _ => Rc::new(Node::Append(a, b)),
    }
}

pub fn group(d: Doc) -> Doc {
    match &*d {
        Node::Group(_) | Node::Text(..) | Node::Empty => d,
        _ => Rc::new(Node::Group(d)),
    }
}

pub fn nest(offset: i64, d: Doc) -> Doc {
    if offset == 0 || matches!(&*d, Node::Empty) {
        d
    } else {
        Rc::new(Node::Nest(offset, d))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Break,
    Flat,
}

/// A command: at this indentation, in this mode, lay this out.
type Command = (i64, Mode, Doc);

/// Whether `d` fits in the rest of the line from `pos`, flat, followed by
/// the commands on the stack, broken.
fn fits(d: &Doc, pos: usize, width: usize, stack: &[Command]) -> bool {
    let mut pending = vec![d.clone()];
    let mut below = stack.len();
    let mut mode = Mode::Flat;
    let mut pos = pos;
    loop {
        let Some(next) = pending.pop() else {
            if below == 0 {
                return true;
            }
            below -= 1;
            pending.push(stack[below].2.clone());
            mode = Mode::Break;
            continue;
        };
        match &*next {
            Node::Empty => {}
            Node::Append(l, r) => {
                pending.push(r.clone());
                pending.push(l.clone());
            }
            Node::Hardline => return mode == Mode::Break,
            Node::Text(_, len) => {
                pos += len;
                if pos > width {
                    return false;
                }
            }
            Node::FlatAlt(broken, flat) => pending.push(if mode == Mode::Break { broken.clone() } else { flat.clone() }),
            Node::Nest(_, inner) | Node::Group(inner) => pending.push(inner.clone()),
        }
    }
}

/// `d` laid out in `width` columns.
pub fn render(width: usize, d: Doc) -> String {
    let mut out = String::new();
    let mut pos = 0;
    let mut stack: Vec<Command> = vec![(0, Mode::Break, d)];
    while let Some((mut ind, mut mode, mut doc)) = stack.pop() {
        loop {
            let next = match &*doc {
                Node::Empty => break,
                Node::Append(l, r) => {
                    stack.push((ind, mode, r.clone()));
                    l.clone()
                }
                Node::FlatAlt(broken, flat) => {
                    if mode == Mode::Break {
                        broken.clone()
                    } else {
                        flat.clone()
                    }
                }
                Node::Group(inner) => {
                    if mode == Mode::Break && fits(inner, pos, width, &stack) {
                        mode = Mode::Flat;
                    }
                    inner.clone()
                }
                Node::Nest(offset, inner) => {
                    ind = (ind + offset).max(0);
                    inner.clone()
                }
                Node::Hardline => {
                    let (at, then) = match stack.pop() {
                        Some((at, then_mode, then)) => {
                            mode = then_mode;
                            (at, then)
                        }
                        None => (ind, nil()),
                    };
                    ind = at;
                    out.push('\n');
                    out.extend(std::iter::repeat_n(' ', at.max(0) as usize));
                    pos = at.max(0) as usize;
                    then
                }
                Node::Text(t, len) => {
                    out.push_str(t);
                    pos += len;
                    break;
                }
            };
            doc = next;
        }
    }
    out
}
