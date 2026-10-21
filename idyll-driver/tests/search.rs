//! The REPL's searching, on the real prelude: by name, by type as Hoogle
//! does, proof search, and the pop-ups that show them (drawn into a buffer,
//! and driven with keys).

use idyll::find::{Decl, Index};
use idyll::finder::{Catalog, Picker, Step};
use idyll::viewer::{Tab, Viewer};
use idyllic::pipeline::{self, Options};
use idyllic_diagnostics::OutputStyle;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;

const OPTS: Options = Options { debug: false, style: OutputStyle::PLAIN, live: false };

/// What the finder knows of after the declarations `earlier`.
fn catalog(earlier: &str) -> Catalog {
    let checked = pipeline::check(earlier, &OPTS);
    let decls = Decl::all(&checked.declarations, &|ty| idyll::find::bare(earlier, ty));
    Catalog { index: Index::new(decls), earlier: earlier.to_string(), opts: Some(OPTS) }
}

/// The names of the best `n` answers to `query`.
fn found(c: &Catalog, query: &str, n: usize) -> Vec<String> {
    c.index.search(query, n).iter().map(|h| c.index.decls[h.decl].name.clone()).collect()
}

fn prove(earlier: &str, goal: &str) -> Vec<String> {
    let c = catalog(earlier);
    idyll::prove::prove(earlier, goal, &c.index.decls, &OPTS, 5).unwrap_or_else(|e| panic!("{goal}: {e}"))
}

#[test]
fn a_name_is_found_fuzzily_and_exactly_first() {
    let c = catalog("");
    assert_eq!(found(&c, "showInt", 1), vec!["showInt"]);
    assert_eq!(found(&c, "shwint", 1), vec!["showInt"]);
    assert!(found(&c, "Just", 3).contains(&"Maybe.Just".to_string()));
}

#[test]
fn a_type_finds_what_has_it() {
    let c = catalog("");
    // Hoogle's way: the result has to fit, and the rest is a price
    assert_eq!(found(&c, "Int -> String", 1), vec!["showInt"]);
    assert_eq!(found(&c, "String -> ()", 3).len(), 3);
    assert!(found(&c, ": Maybe a", 5).contains(&"Maybe.Nothing".to_string()));
}

#[test]
fn the_sessions_own_declarations_are_found_and_documented() {
    let c = catalog("-- twice what it is given\ndouble : Int -> Int\ndouble n = n * 2\n");
    let hits = c.index.search("Int -> Int", 5);
    let d = &c.index.decls[hits[0].decl];
    assert_eq!(d.name, "double");
    assert_eq!(d.doc.as_deref(), Some("twice what it is given"));
    assert_eq!(d.location.as_ref().map(|(_, line)| *line), Some(2));
    assert!(!d.prelude);
}

#[test]
fn a_constructor_is_spelled_as_the_prompt_can_write_it() {
    let c = catalog("data Nat = Z | S Nat\n");
    let spelling = |name: &str| c.index.decls.iter().find(|d| d.name == name).map(|d| d.spelling.clone());
    // not `use`d: under its type
    assert_eq!(spelling("Nat.Z").as_deref(), Some("Nat.Z"));
    // the prelude's `use`s put these in scope bare
    assert_eq!(spelling("Maybe.Just").as_deref(), Some("Just"));
    assert_eq!(spelling("List.::").as_deref(), Some("(::)"));
}

#[test]
fn proof_search_finds_terms_the_checker_accepts() {
    // the identity is the proof of `a -> a`
    assert_eq!(prove("", "{a : Type} -> a -> a").first().map(String::as_str), Some("\\x -> x"));
    // a name whose type is the goal is the smallest proof of it
    assert!(prove("", "(n : [] Int) -> String").contains(&"showInt".to_string()));
    // a constructor applied to what it needs
    let maybe = prove("", "Maybe Int");
    assert!(maybe.contains(&"Nothing".to_string()), "{maybe:?}");
    assert!(maybe.iter().any(|t| t.starts_with("Just ")), "{maybe:?}");
}

#[test]
fn proof_search_respects_linearity() {
    // `\x y -> x` drops `y`, which a linear argument may not be
    let found = prove("", "{a : Type} -> {b : Type} -> a -> (y : [1] b) -> a");
    assert!(found.is_empty(), "{found:?}");
    // with `y` allowed to go unused, it is a proof: as one that says nothing
    // is, outside a linear module
    assert_eq!(prove("", "{a : Type} -> {b : Type} -> a -> b -> a").first().map(String::as_str), Some("\\x y -> x"));
    assert_eq!(
        prove("", "{a : Type} -> {b : Type} -> a -> (y : [0..1] b) -> a").first().map(String::as_str),
        Some("\\x y -> x")
    );
}

#[test]
fn proof_search_uses_the_sessions_declarations() {
    let earlier = "data Nat = Z | S Nat\n\nuse Nat.*\n\ndouble : Nat -> Nat\ndouble Z = Z\ndouble (S n) = S (S (double n))\n";
    let found = prove(earlier, "Nat -> Nat");
    assert!(found.contains(&"double".to_string()), "{found:?}");
    // `use Nat.*` put `S` in scope bare
    assert!(found.contains(&"S".to_string()), "{found:?}");
}

#[test]
fn a_goal_that_is_not_a_type_says_why() {
    let c = catalog("");
    let err = idyll::prove::prove("", "Nonsense -> a", &c.index.decls, &OPTS, 5).expect_err("not a type");
    assert!(err.contains("Nonsense"), "{err}");
}

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn typed(picker: &mut Picker, text: &str) {
    for c in text.chars() {
        picker.key(press(KeyCode::Char(c)));
    }
}

/// What a frame drew, as lines of text.
fn screen(draw: impl FnOnce(&mut ratatui::Frame)) -> String {
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).expect("a terminal");
    let frame = terminal.draw(draw).expect("drawn");
    let buffer = frame.buffer;
    (0..buffer.area.height)
        .map(|y| (0..buffer.area.width).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_finder_narrows_as_it_is_typed_and_inserts_the_spelling() {
    let c = catalog("");
    let mut picker = Picker::new(&c).plain();
    typed(&mut picker, "Int -> String");
    assert_eq!(picker.listed().next().map(|d| d.name.as_str()), Some("showInt"));
    let drawn = screen(|f| picker.draw(f));
    assert!(drawn.contains("by type"), "{drawn}");
    assert!(drawn.contains("showInt"), "{drawn}");
    assert!(drawn.contains("from    the prelude"), "{drawn}");
    assert_eq!(picker.key(press(KeyCode::Enter)), Step::Choose("showInt".into()));
}

#[test]
fn the_finder_searches_for_a_proof_on_enter_and_inserts_it_on_the_next() {
    let c = catalog("");
    let mut picker = Picker::new(&c).plain();
    typed(&mut picker, "? {a : Type} -> a -> a");
    assert!(screen(|f| picker.draw(f)).contains("↵ search"));
    assert_eq!(picker.key(press(KeyCode::Enter)), Step::Continue);
    assert_eq!(picker.proofs().and_then(|p| p.first()).map(String::as_str), Some("\\x -> x"));
    assert!(screen(|f| picker.draw(f)).contains("checked: a term of the goal"));
    assert_eq!(picker.key(press(KeyCode::Enter)), Step::Choose("\\x -> x".into()));
    // typing again asks again
    typed(&mut picker, " ");
    assert!(picker.proofs().is_none());
}

#[test]
fn the_viewer_moves_between_stages_and_scrolls() {
    let tab = |name: &str, lines: usize| Tab {
        name: name.into(),
        about: format!("about {name}"),
        text: (0..lines).map(|i| format!("{name} line {i}")).collect::<Vec<_>>().join("\n"),
    };
    let mut viewer = Viewer::new("double n = n * 2", vec![tab("tokens", 3), tab("core", 200)]).plain();
    let drawn = screen(|f| viewer.draw(f));
    assert!(drawn.contains("1 tokens") && drawn.contains("2 core"), "{drawn}");
    assert!(drawn.contains("tokens line 0"), "{drawn}");
    viewer.key(press(KeyCode::Right));
    assert_eq!(viewer.showing().name, "core");
    viewer.key(press(KeyCode::PageDown));
    let drawn = screen(|f| viewer.draw(f));
    assert!(!drawn.contains("core line 0 "), "{drawn}");
    assert!(drawn.contains("of 200"), "{drawn}");
    viewer.key(press(KeyCode::Char('1')));
    assert_eq!(viewer.showing().name, "tokens");
    assert!(viewer.key(press(KeyCode::Esc)));
}
