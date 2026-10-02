//! **The declaration finder**: `Ctrl-F` in the REPL.  Ported from Meadow's.
//!
//! A pop-up in the manner of Helix's pickers: a query at the top, what it
//! matches below, and on the right everything known about the one selected
//! -- its type, where it was written, its doc comment.  Typing narrows the
//! list; `Enter` puts the name on the prompt where the cursor was.
//!
//! The query is a name, matched fuzzily, or a type, matched the way Hoogle
//! matches one ([`crate::find`]).  A query starting with `?` is a type to
//! find a *term* of: `Enter` runs the proof search ([`crate::prove`]) and
//! lists what the checker accepted, and `Enter` again puts the one selected
//! on the prompt.
//!
//! [`Picker`] takes keys and draws frames and knows nothing of a terminal;
//! [`pick`] is the few lines that give it one.  The picker is tested by
//! driving it with keys and reading what it drew into a buffer.

use crate::find::{Decl, Hit, Index, Kind, Query};
use idyllic::pipeline::Options;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

/// What the finder searches, and what proof search checks its terms with.
#[derive(Default)]
pub struct Catalog {
    pub index: Index,
    /// the session's declarations, which a term found may use
    pub earlier: String,
    pub opts: Option<Options>,
}

/// What a key did.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Continue,
    /// this goes on the prompt, at the cursor
    Choose(String),
    Cancel,
}

/// What a proof search found, for the query it was run for.
struct Proofs {
    query: String,
    found: Result<Vec<String>, String>,
}

/// The finder, apart from any terminal.
pub struct Picker<'a> {
    catalog: &'a Catalog,
    query: String,
    /// in characters, not bytes
    cursor: usize,
    hits: Vec<Hit>,
    proofs: Option<Proofs>,
    list: ListState,
    /// no colour: `NO_COLOR` is set, or a test is reading the text
    plain: bool,
}

impl<'a> Picker<'a> {
    pub fn new(catalog: &'a Catalog) -> Picker<'a> {
        let mut p = Picker {
            catalog,
            query: String::new(),
            cursor: 0,
            hits: Vec::new(),
            proofs: None,
            list: ListState::default(),
            plain: std::env::var_os("NO_COLOR").is_some(),
        };
        p.refresh();
        p
    }

    /// Without colour, whatever the environment says.
    pub fn plain(mut self) -> Self {
        self.plain = true;
        self
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    /// The type a term is asked for, if the query asks for one.
    fn goal(&self) -> Option<&str> {
        self.query.trim_start().strip_prefix('?').map(str::trim)
    }

    /// The proofs found for the query as it now is.
    fn current_proofs(&self) -> Option<&Result<Vec<String>, String>> {
        self.proofs.as_ref().filter(|p| p.query == self.query).map(|p| &p.found)
    }

    /// How many rows the list has.
    fn rows(&self) -> usize {
        match (self.goal(), self.current_proofs()) {
            (Some(_), Some(Ok(found))) => found.len(),
            (Some(_), _) => 0,
            (None, _) => self.hits.len(),
        }
    }

    /// The declaration selected, if the list has any.
    pub fn selected(&self) -> Option<&'a Decl> {
        if self.goal().is_some() {
            return None;
        }
        let hit = self.hits.get(self.list.selected()?)?;
        Some(&self.catalog.index.decls[hit.decl])
    }

    /// The declarations listed, best first.
    pub fn listed(&self) -> impl Iterator<Item = &'a Decl> + '_ {
        self.hits.iter().map(|h| &self.catalog.index.decls[h.decl])
    }

    /// The terms listed, if a proof search has run for the query.
    pub fn proofs(&self) -> Option<&[String]> {
        match self.current_proofs() {
            Some(Ok(found)) => Some(found),
            _ => None,
        }
    }

    fn refresh(&mut self) {
        self.hits = if self.goal().is_some() { Vec::new() } else { self.catalog.index.search(&self.query, usize::MAX) };
        self.select_first();
    }

    fn select_first(&mut self) {
        self.list.select(if self.rows() == 0 { None } else { Some(0) });
        *self.list.offset_mut() = 0;
    }

    fn prove(&mut self) {
        let goal = self.goal().unwrap_or("").to_string();
        let opts = self.catalog.opts.unwrap_or(Options {
            debug: false,
            style: idyllic_diagnostics::OutputStyle::PLAIN,
            live: false,
        });
        let found = crate::prove::prove(&self.catalog.earlier, &goal, &self.catalog.index.decls, &opts, 20);
        self.proofs = Some(Proofs { query: self.query.clone(), found });
        self.select_first();
    }

    fn byte_at(&self, chars: usize) -> usize {
        self.query.char_indices().nth(chars).map_or(self.query.len(), |(i, _)| i)
    }

    fn edit(&mut self, f: impl FnOnce(&mut Self)) {
        let before = self.query.clone();
        f(self);
        if self.query != before {
            self.refresh();
        }
    }

    fn step_selection(&mut self, by: isize) {
        let n = self.rows();
        if n == 0 {
            return;
        }
        let at = self.list.selected().unwrap_or(0) as isize;
        // wraps, as Helix's does: past the bottom is the top
        let next = (at + by).rem_euclid(n as isize);
        self.list.select(Some(next as usize));
    }

    pub fn key(&mut self, key: KeyEvent) -> Step {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return Step::Cancel,
            KeyCode::Char('c' | 'f') if ctrl => return Step::Cancel,
            KeyCode::Enter if self.goal().is_some() => {
                // the first Enter searches; once it has, Enter chooses
                match (self.proofs(), self.list.selected()) {
                    (Some(found), Some(i)) if i < found.len() => return Step::Choose(found[i].clone()),
                    (Some(_), _) => {}
                    (None, _) => self.prove(),
                }
            }
            KeyCode::Enter => {
                return match self.selected() {
                    Some(d) => Step::Choose(d.spelling.clone()),
                    None => Step::Continue,
                };
            }
            KeyCode::Up | KeyCode::BackTab => self.step_selection(-1),
            KeyCode::Down | KeyCode::Tab => self.step_selection(1),
            KeyCode::Char('p' | 'k') if ctrl => self.step_selection(-1),
            KeyCode::Char('n' | 'j') if ctrl => self.step_selection(1),
            KeyCode::PageUp => self.step_selection(-10),
            KeyCode::PageDown => self.step_selection(10),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.query.chars().count()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.query.chars().count(),
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::Char('e') if ctrl => self.cursor = self.query.chars().count(),
            KeyCode::Char('u') if ctrl => self.edit(|p| {
                let at = p.byte_at(p.cursor);
                p.query.replace_range(..at, "");
                p.cursor = 0;
            }),
            KeyCode::Char('w') if ctrl => self.edit(|p| {
                let end = p.byte_at(p.cursor);
                let head = &p.query[..end];
                let trimmed = head.trim_end();
                let start = trimmed.rfind(|c: char| c.is_whitespace()).map_or(0, |i| i + 1);
                p.cursor -= head[start..].chars().count();
                p.query.replace_range(start..end, "");
            }),
            KeyCode::Backspace => self.edit(|p| {
                if p.cursor > 0 {
                    let at = p.byte_at(p.cursor - 1);
                    p.query.remove(at);
                    p.cursor -= 1;
                }
            }),
            KeyCode::Delete => self.edit(|p| {
                if p.cursor < p.query.chars().count() {
                    let at = p.byte_at(p.cursor);
                    p.query.remove(at);
                }
            }),
            KeyCode::Char(c) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => self.edit(|p| {
                let at = p.byte_at(p.cursor);
                p.query.insert(at, c);
                p.cursor += 1;
            }),
            _ => {}
        }
        Step::Continue
    }

    // --- drawing -----------------------------------------------------------

    fn style(&self, s: Style) -> Style {
        if self.plain { Style::default() } else { s }
    }

    fn kind_colour(kind: Kind) -> Color {
        match kind {
            Kind::Function | Kind::Value | Kind::Operation => Color::Blue,
            Kind::Constructor => Color::Green,
            Kind::Type | Kind::Alias => Color::Yellow,
            Kind::Effect => Color::Red,
        }
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let area = popup(frame.area());
        frame.render_widget(Clear, area);
        // a preview needs room beside the list; on a narrow terminal the
        // list is what matters
        let (left, right) = if area.width >= 90 {
            let [l, r] = Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);
            (l, Some(r))
        } else {
            (area, None)
        };
        self.draw_list(frame, left);
        if let Some(right) = right {
            self.draw_preview(frame, right);
        }
    }

    fn draw_list(&mut self, frame: &mut Frame, area: Rect) {
        let proving = self.goal().is_some();
        let mode = if proving {
            " a term of a type "
        } else if Query::parse(&self.query).is_type() {
            " by type "
        } else {
            " by name "
        };
        let keys = if proving && self.proofs().is_none() {
            " ↵ search · esc close "
        } else {
            " ↵ insert · ↑↓ move · esc close "
        };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(Line::from(" Find a declaration ").style(self.style(Style::new().bold())))
            .title(Line::from(mode).right_aligned().style(self.style(Style::new().dim())))
            .title_bottom(Line::from(keys).style(self.style(Style::new().dim())));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let [prompt, rule, rows] =
            Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(0)]).areas(inner);

        // the prompt, with the count on the right
        let count = if proving {
            match self.proofs() {
                Some(found) => format!("{} found ", found.len()),
                None => String::new(),
            }
        } else {
            format!("{}/{} ", self.hits.len(), self.catalog.index.len())
        };
        let [text, tally] =
            Layout::horizontal([Constraint::Min(0), Constraint::Length(count.chars().count() as u16)]).areas(prompt);
        let shown = if self.query.is_empty() {
            Line::from(vec![
                Span::styled("❯ ", self.style(Style::new().fg(Color::Cyan))),
                Span::styled("a name, a type like  Int -> String, or ? and a type to prove", self.style(Style::new().dim())),
            ])
        } else {
            Line::from(vec![Span::styled("❯ ", self.style(Style::new().fg(Color::Cyan))), Span::raw(self.query.clone())])
        };
        frame.render_widget(Paragraph::new(shown), text);
        frame.render_widget(Paragraph::new(count).style(self.style(Style::new().dim())), tally);
        frame.set_cursor_position(Position::new(
            text.x + 2 + self.query[..self.byte_at(self.cursor)].chars().count() as u16,
            text.y,
        ));
        frame.render_widget(Paragraph::new("─".repeat(rule.width as usize)).style(self.style(Style::new().dim())), rule);

        let width = (rows.width as usize).saturating_sub(1); // the `▌`
        let items: Vec<ListItem> = if proving {
            self.proofs()
                .unwrap_or(&[])
                .iter()
                .map(|term| {
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{:<7}", "proof"), self.style(Style::new().fg(Color::Magenta))),
                        Span::styled(truncate(term, width.saturating_sub(7)), self.style(Style::new().bold())),
                    ]))
                })
                .collect()
        } else {
            // columns: what it is, its name, its type, and -- at the right,
            // dim -- where it is from
            let decls: Vec<&Decl> = self.hits.iter().map(|h| &self.catalog.index.decls[h.decl]).collect();
            let name_w = decls.iter().map(|d| d.name.chars().count()).max().unwrap_or(0).min(24);
            let tag_w = decls.iter().map(|d| d.module.chars().count()).max().unwrap_or(0).min(16);
            decls
                .iter()
                .map(|d| {
                    let label = format!("{:<7}", d.kind.label());
                    let name = format!("{:<name_w$}", truncate(&d.name, name_w));
                    let fixed = 7 + name_w + 2 + 2 + tag_w;
                    let room = width.saturating_sub(fixed);
                    let detail = truncate(&d.detail, room);
                    let pad = room.saturating_sub(detail.chars().count());
                    ListItem::new(Line::from(vec![
                        Span::styled(label, self.style(Style::new().fg(Self::kind_colour(d.kind)))),
                        Span::styled(name, self.style(Style::new().bold())),
                        Span::raw("  "),
                        Span::raw(detail),
                        Span::raw(" ".repeat(pad + 2)),
                        Span::styled(format!("{:>tag_w$}", truncate(&d.module, tag_w)), self.style(Style::new().dim())),
                    ]))
                })
                .collect()
        };
        let highlight =
            if self.plain { Style::new().add_modifier(Modifier::REVERSED) } else { Style::new().bg(Color::DarkGray) };
        let list = List::new(items)
            .highlight_style(highlight)
            .highlight_symbol("▌")
            .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);
        frame.render_stateful_widget(list, rows, &mut self.list);
    }

    fn draw_preview(&self, frame: &mut Frame, area: Rect) {
        let dim = self.style(Style::new().dim());
        if let Some(goal) = self.goal() {
            let block = Block::bordered().border_type(BorderType::Rounded).title(" Proof search ");
            let mut lines = vec![
                Line::from(vec![Span::styled("goal    ", dim), Span::raw(goal.to_string())]),
                Line::raw(""),
            ];
            match self.current_proofs() {
                None => {
                    lines.push(Line::raw("↵ looks for terms of this type: lambdas, and what is in"));
                    lines.push(Line::raw("scope applied to what it needs, the smallest first."));
                    lines.push(Line::raw(""));
                    lines.push(Line::from(Span::styled("Every term listed has been checked.", dim)));
                }
                Some(Ok(found)) if found.is_empty() => {
                    lines.push(Line::raw("Nothing found: a goal that needs a `case` is beyond"));
                    lines.push(Line::raw("the search, and so is one nested deeper than three."));
                }
                Some(Ok(found)) => {
                    let term = self.list.selected().and_then(|i| found.get(i)).cloned().unwrap_or_default();
                    lines.push(Line::from(Span::styled(
                        term,
                        self.style(Style::new().fg(Color::Magenta).bold()),
                    )));
                    lines.push(Line::raw(""));
                    lines.push(Line::from(Span::styled("checked: a term of the goal", dim)));
                }
                Some(Err(why)) => {
                    lines.extend(why.lines().map(|l| Line::raw(l.to_string())));
                }
            }
            frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(block), area);
            return;
        }
        let Some(d) = self.selected() else {
            let empty = Block::bordered().border_type(BorderType::Rounded);
            let hint = if self.query.is_empty() { "Nothing to show." } else { "Nothing matches." };
            frame.render_widget(Paragraph::new(hint).style(dim).block(empty), area);
            return;
        };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(Line::from(format!(" {} ", d.qualified())).style(self.style(Style::new().bold())));
        let mut lines = vec![
            Line::from(Span::styled(d.headline(), self.style(Style::new().fg(Self::kind_colour(d.kind)).bold()))),
            Line::raw(""),
            Line::from(vec![
                Span::styled("from    ", dim),
                Span::raw(match d.module.as_str() {
                    "" => "this session".to_string(),
                    "Prelude" => "the prelude".to_string(),
                    package => format!("the {package} package"),
                }),
            ]),
        ];
        if let Some((file, line)) = &d.location {
            lines.push(Line::from(vec![Span::styled("where   ", dim), Span::raw(format!("{file}:{line}"))]));
        }
        lines.push(Line::from(vec![Span::styled("written ", dim), Span::raw(d.spelling.clone())]));
        if let Some(doc) = &d.doc {
            lines.push(Line::raw(""));
            lines.extend(doc.lines().map(|l| Line::raw(l.to_string())));
        }
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).block(block), area);
    }
}

/// Where a pop-up goes: most of the screen, centred, as Helix's is -- and
/// all of it when there is little of it.
pub fn popup(screen: Rect) -> Rect {
    let width = if screen.width < 60 { screen.width } else { (screen.width * 9 / 10).min(160) };
    let height = if screen.height < 16 { screen.height } else { (screen.height * 8 / 10).min(48) };
    Rect { x: screen.x + (screen.width - width) / 2, y: screen.y + (screen.height - height) / 2, width, height }
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    s.chars().take(max - 1).collect::<String>() + "…"
}

/// Run a pop-up over the terminal until it says it is done, on the
/// alternate screen, so that what was on the screen before comes back
/// exactly as it was.  `draw` draws it, and `key` answers each key: `Some`
/// when it is done.
///
/// With `raw`, the terminal is put in raw mode for the pop-up and back
/// after.  Without, it is left alone: the finder is opened from inside the
/// line editor, whose raw mode is what it needs, and putting the terminal
/// back afterwards would be putting it back into the wrong one (crossterm
/// cannot see the line editor's raw mode, and on Windows would turn the
/// console's line input back on under it).
pub fn run_popup<T>(
    raw: bool,
    mut draw: impl FnMut(&mut Frame),
    mut key: impl FnMut(KeyEvent) -> Option<T>,
) -> std::io::Result<T> {
    use ratatui::crossterm::{cursor, event, execute, terminal};
    if raw {
        terminal::enable_raw_mode()?;
    }
    let mut out = std::io::stdout();
    execute!(out, terminal::EnterAlternateScreen)?;
    let result = (|| {
        let mut terminal = ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))?;
        terminal.clear()?;
        loop {
            terminal.draw(&mut draw)?;
            match event::read()? {
                // a key is reported pressed and released on some platforms;
                // only the press is a keystroke
                event::Event::Key(k) if k.kind != KeyEventKind::Release => {
                    if let Some(done) = key(k) {
                        return Ok(done);
                    }
                }
                _ => {}
            }
        }
    })();
    execute!(out, cursor::Show, terminal::LeaveAlternateScreen)?;
    if raw {
        terminal::disable_raw_mode()?;
    }
    result
}

/// Show the finder over the terminal, and answer what was chosen.
pub fn pick(catalog: &Catalog) -> std::io::Result<Option<String>> {
    let picker = std::cell::RefCell::new(Picker::new(catalog));
    run_popup(
        false,
        |f| picker.borrow_mut().draw(f),
        |k| match picker.borrow_mut().key(k) {
            Step::Continue => None,
            Step::Choose(c) => Some(Some(c)),
            Step::Cancel => Some(None),
        },
    )
}
