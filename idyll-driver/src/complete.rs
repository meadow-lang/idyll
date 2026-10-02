//! **Tab completion** in the REPL: a command after `:`, a stage after
//! `:ir`, and otherwise a keyword or a name -- the session's and the
//! prelude's, as the prompt spells them, constructors under their types
//! (`Nat.Z`) as well as bare where they are in scope bare.

use crate::find::Decl;

/// What can be completed, refreshed after every entry.
#[derive(Default, Clone)]
pub struct Names {
    names: Vec<String>,
    commands: Vec<String>,
}

const KEYWORDS: &[&str] = &[
    "data", "effect", "where", "case", "of", "if", "then", "else", "let", "in", "handle", "with", "use", "pub", "Type",
    "True", "False",
];

impl Names {
    pub fn new(decls: &[Decl], commands: &[&str]) -> Names {
        let mut names: Vec<String> = decls.iter().flat_map(|d| [d.name.clone(), d.spelling.clone()]).collect();
        names.extend(KEYWORDS.iter().map(|k| k.to_string()));
        // an operator is not something to complete a word into
        names.retain(|n| n.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '_'));
        names.sort();
        names.dedup();
        Names { names, commands: commands.iter().map(|c| c.to_string()).collect() }
    }

    /// What `prefix`, the word before the cursor, may be completed to, in a
    /// line that reads `head` up to the cursor.
    pub fn candidates(&self, head: &str, prefix: &str) -> Vec<String> {
        let line = head.trim_start();
        // a command
        if line.starts_with(':') && !line.contains(char::is_whitespace) {
            return self.commands.iter().filter(|c| c.starts_with(line)).cloned().collect();
        }
        // a stage, or stages, for `:ir`
        if let Some(rest) = line.strip_prefix(":ir ") {
            if !rest.contains(char::is_whitespace) {
                let last = rest.rsplit(',').next().unwrap_or("");
                return idyllic::inspect::STAGES
                    .iter()
                    .map(|(name, _)| *name)
                    .filter(|n| n.starts_with(last))
                    .map(str::to_string)
                    .collect();
            }
        }
        if prefix.is_empty() {
            return Vec::new();
        }
        self.names.iter().filter(|n| n.starts_with(prefix)).cloned().collect()
    }
}

/// Where the word the cursor is at the end of starts: back over what a name
/// is made of, dots included (`Nat.Z`).  After `:ir `, a stage list's last
/// stage.
pub fn word_start(line: &str, pos: usize) -> usize {
    let head = &line[..pos];
    head.char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || matches!(c, '_' | '\'' | '.' | ':'))
        .last()
        .map_or(pos, |(i, _)| i)
        .max(head.rfind(',').map_or(0, |i| i + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Names {
        let decl = |name: &str, spelling: &str| Decl {
            name: name.into(),
            spelling: spelling.into(),
            module: String::new(),
            kind: crate::find::Kind::Function,
            detail: String::new(),
            shape: None,
            doc: None,
            prelude: false,
            location: None,
        };
        Names::new(&[decl("showInt", "showInt"), decl("Maybe.Just", "Just"), decl("(++)", "(++)")], &[":ir", ":reset"])
    }

    #[test]
    fn a_name_completes_from_what_is_declared() {
        let n = names();
        assert_eq!(n.candidates("1 + sho", "sho"), vec!["showInt"]);
        assert_eq!(n.candidates("Ju", "Ju"), vec!["Just"]);
        assert_eq!(n.candidates("Maybe.", "Maybe."), vec!["Maybe.Just"]);
        assert!(n.candidates("x ++ (", "").is_empty());
    }

    #[test]
    fn a_command_and_a_stage_complete_too() {
        let n = names();
        assert_eq!(n.candidates(":r", ":r"), vec![":reset"]);
        assert_eq!(n.candidates(":ir co", "co"), vec!["core"]);
        assert_eq!(n.candidates(":ir core,cu", "cu"), vec!["cut"]);
    }

    #[test]
    fn the_word_starts_after_what_cannot_be_in_a_name() {
        assert_eq!(word_start("1 + sho", 7), 4);
        assert_eq!(word_start("Nat.Z", 5), 0);
        assert_eq!(word_start(":ir core,cu", 11), 9);
    }
}
