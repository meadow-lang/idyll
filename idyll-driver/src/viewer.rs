//! **The IR viewer**: `:ir` in the REPL.
//!
//! A pop-up like the finder's, with a tab for each stage of the compiler
//! (see `idyllic::inspect`): what the block of code is as tokens, as a syntax
//! tree, as HIR, as Core, as AxCut (lowered, then inlined), as LLVM IR, and
//! as Cut, the shared back end's IR.  `←`/`→` (or `Tab`, or a stage's number)
//! move between them, and `↑`/`↓`, `PgUp`/`PgDn`, `Home`/`End` scroll.
//!
//! [`Viewer`] takes keys and draws frames and knows nothing of a terminal,
//! as the finder's [`crate::finder::Picker`] does.

use crate::finder::{popup, run_popup};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Tabs};
use ratatui::Frame;

/// One tab: a stage's name, what it is, and its text.
pub struct Tab {
    pub name: String,
    pub about: String,
    pub text: String,
}

pub struct Viewer {
    /// what is being shown, for the title: the block's first line
    title: String,
    tabs: Vec<Tab>,
    at: usize,
    /// how far each tab is scrolled
    scroll: Vec<usize>,
    /// how many lines of text the last frame had room for
    height: usize,
    plain: bool,
}

impl Viewer {
    pub fn new(title: &str, tabs: Vec<Tab>) -> Viewer {
        let n = tabs.len();
        Viewer {
            title: title.to_string(),
            tabs,
            at: 0,
            scroll: vec![0; n],
            height: 20,
            plain: std::env::var_os("NO_COLOR").is_some(),
        }
    }

    /// Without colour, whatever the environment says.
    pub fn plain(mut self) -> Self {
        self.plain = true;
        self
    }

    /// The tab showing.
    pub fn showing(&self) -> &Tab {
        &self.tabs[self.at]
    }

    fn style(&self, s: Style) -> Style {
        if self.plain { Style::default() } else { s }
    }

    fn lines(&self) -> usize {
        self.tabs.get(self.at).map_or(0, |t| t.text.lines().count())
    }

    fn scroll_by(&mut self, by: isize) {
        let most = self.lines().saturating_sub(self.height);
        let s = &mut self.scroll[self.at];
        *s = (*s as isize + by).clamp(0, most as isize) as usize;
    }

    fn switch(&mut self, by: isize) {
        let n = self.tabs.len() as isize;
        if n > 0 {
            self.at = (self.at as isize + by).rem_euclid(n) as usize;
        }
    }

    /// `true` when the viewer is to close.
    pub fn key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let page = self.height.max(1) as isize;
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return true,
            KeyCode::Char('c') if ctrl => return true,
            KeyCode::Right | KeyCode::Tab | KeyCode::Char('l') => self.switch(1),
            KeyCode::Left | KeyCode::BackTab | KeyCode::Char('h') => self.switch(-1),
            KeyCode::Char(d @ '1'..='9') => {
                let i = d as usize - '1' as usize;
                if i < self.tabs.len() {
                    self.at = i;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => self.scroll_by(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_by(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.scroll_by(page),
            KeyCode::PageUp => self.scroll_by(-page),
            KeyCode::Char('d') if ctrl => self.scroll_by(page / 2),
            KeyCode::Char('u') if ctrl => self.scroll_by(-page / 2),
            KeyCode::Home | KeyCode::Char('g') => self.scroll[self.at] = 0,
            KeyCode::End | KeyCode::Char('G') => self.scroll_by(isize::MAX / 2),
            _ => {}
        }
        false
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let area = popup(frame.area());
        frame.render_widget(Clear, area);
        let dim = self.style(Style::new().dim());
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(Line::from(format!(" :ir {} ", crate::finder::truncate(&self.title, 60))).style(self.style(Style::new().bold())))
            .title_bottom(Line::from(" ←→ stage · ↑↓ scroll · esc close ").style(dim));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let [tabs, about, rule, body] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(inner);
        let titles: Vec<String> = self.tabs.iter().enumerate().map(|(i, t)| format!("{} {}", i + 1, t.name)).collect();
        frame.render_widget(
            Tabs::new(titles)
                .select(self.at)
                .style(dim)
                .highlight_style(self.style(Style::new().fg(Color::Cyan).bold()))
                .divider("·"),
            tabs,
        );
        let Some(tab) = self.tabs.get(self.at) else { return };
        let lines = tab.text.lines().count();
        self.height = body.height as usize;
        let at = self.scroll[self.at];
        let position = if lines > self.height { format!("  {}–{} of {lines}", at + 1, (at + self.height).min(lines)) } else { String::new() };
        frame.render_widget(Paragraph::new(format!(" {}{position}", tab.about)).style(dim), about);
        frame.render_widget(Paragraph::new("─".repeat(rule.width as usize)).style(dim), rule);
        frame.render_widget(Paragraph::new(tab.text.as_str()).scroll((at as u16, 0)), body);
    }
}

/// Show the viewer over the terminal until it is closed.
pub fn show(viewer: Viewer) -> std::io::Result<()> {
    let viewer = std::cell::RefCell::new(viewer);
    // shown from the prompt, between entries, when the line editor has put
    // the terminal back as it was
    run_popup(true, |f| viewer.borrow_mut().draw(f), |k| viewer.borrow_mut().key(k).then_some(()))
}
