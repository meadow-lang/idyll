#![allow(dead_code, non_snake_case, non_upper_case_globals, unused_imports, unexpected_cfgs)]
mod uw_tables;
mod us_tables;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

fn raw_width(cp: usize) -> u8 {
    use uw_tables::*;
    let t1_offset = WIDTH_ROOT.0[cp >> 13];
    let t2_offset = WIDTH_MIDDLE.0[usize::from(t1_offset)][cp >> 7 & 0x3F];
    let packed_widths = WIDTH_LEAVES.0[usize::from(t2_offset)][cp >> 2 & 0x1F];
    packed_widths >> (2 * (cp & 0b11)) & 0b11
}

fn chars() -> impl Iterator<Item = char> {
    (0u32..=0x10FFFF).filter_map(char::from_u32)
}

/// Emit maximal runs of equal values (surrogates are skipped, so a run may
/// span them; they can never occur in Haskell `Text`).
fn runs<V: PartialEq + Copy, F: Fn(char) -> V>(tag: &str, f: F, emit_default: Option<V>, show: fn(V) -> String) {
    let mut cur: Option<(u32, u32, V)> = None;
    for c in chars() {
        let v = f(c);
        let cp = c as u32;
        match cur {
            Some((lo, hi, w)) if w == v && (hi + 1 == cp || (hi == 0xD7FF && cp == 0xE000)) => {
                cur = Some((lo, cp, w));
            }
            _ => {
                if let Some((lo, hi, w)) = cur {
                    if Some(w) != emit_default { println!("{tag} {lo:X} {hi:X} {}", show(w)); }
                }
                cur = Some((cp, cp, v));
            }
        }
    }
    if let Some((lo, hi, w)) = cur {
        if Some(w) != emit_default { println!("{tag} {lo:X} {hi:X} {}", show(w)); }
    }
}

fn gcat(c: char) -> us_tables::grapheme::GraphemeCat {
    use us_tables::grapheme::GraphemeCat::*;
    if c <= '\u{7e}' {
        if c >= '\u{20}' { GC_Any } else if c == '\n' { GC_LF } else if c == '\r' { GC_CR } else { GC_Control }
    } else {
        us_tables::grapheme::grapheme_category(c).2
    }
}

fn esc(s: &str) -> String {
    s.chars().map(|c| format!("{:X}", c as u32)).collect::<Vec<_>>().join(",")
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    if mode == "tables" {
        runs("W", |c| raw_width(c as usize), Some(1), |v| v.to_string());
        for (lo, hi) in uw_tables::NON_TRANSPARENT_ZERO_WIDTHS.iter() {
            let lo = u32::from_le_bytes([lo[0], lo[1], lo[2], 0]);
            let hi = u32::from_le_bytes([hi[0], hi[1], hi[2], 0]);
            println!("NTZ {lo:X} {hi:X} 1");
        }
        runs("EP", uw_tables::starts_emoji_presentation_seq, Some(false), |_| "1".into());
        runs("TP", uw_tables::starts_non_ideographic_text_presentation_seq, Some(false), |_| "1".into());
        runs("EMB", uw_tables::is_emoji_modifier_base, Some(false), |_| "1".into());
        runs("GC", gcat, Some(us_tables::grapheme::GraphemeCat::GC_Any), |v| format!("{v:?}"));
        runs("ICE", us_tables::derived_property::InCB_Extend, Some(false), |_| "1".into());
        runs("ICL", us_tables::is_incb_linker, Some(false), |_| "1".into());
        // Rust std predicates rustyline relies on (they differ from GHC's
        // Data.Char in a handful of code points).
        runs("ALNUM", char::is_alphanumeric, Some(false), |_| "1".into());
        runs("WS", char::is_whitespace, Some(false), |_| "1".into());
    } else {
        // oracle: one line per sample: codepoints | str width | char widths | grapheme byte offsets
        let mut samples: Vec<String> = vec![
            "", "a", "abc", "héllo", "e\u{301}", "👩‍🚀", "👋🏿", "👨‍👩‍👧‍👦", "👩🏼‍👨🏼‍👦🏼‍👦🏼", "❤️", "❤", "❤\u{FE0E}",
            "🇫🇷🇩🇪", "🇫🇷🇩", "🇦", "日本語", "ｱｲｳ", "\t", "\r\n", "a\r\nb", "\n", "x\u{200D}y",
            "क्ष", "हिन्दी", "क्‍ष", "ᄀ각", "각", "1️⃣", "#\u{FE0F}\u{20E3}", "\u{0644}\u{0627}",
            "\u{05D0}\u{200D}\u{05DC}", "🏴\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}",
            "\u{17D2}", "ក្ក", "\u{1F3FB}", "a\u{1F3FB}", "\u{0600}a", "\u{1100}\u{1161}\u{11A8}",
            "\u{2018}\u{FE01}", "©", "©\u{FE0F}", "👨‍👩", "🇦🇧🇨", "\u{7F}", "\u{80}\u{9F}", "\u{A0}",
            "\u{AD}", "\u{200B}", "\u{2D31}\u{2D7F}\u{2D31}", "ꓸ", "\u{A4FC}\u{A4F8}",
            "\u{0915}\u{094D}\u{200D}\u{0937}", "\u{0915}\u{093C}\u{094D}\u{0937}",
            "🧑\u{200D}🧑\u{200D}🧒", "😀\u{200D}\u{200D}😀", "\u{1F1E6}\u{200D}\u{1F1E6}",
        ].into_iter().map(String::from).collect();
        // exhaustive-ish: every pair drawn from an interesting alphabet
        let alphabet: Vec<char> = vec!['a', '\u{301}', '\u{200D}', '\u{FE0F}', '\u{FE0E}', '👩', '\u{1F3FB}',
            '\u{1F1E6}', '\r', '\n', '\u{0915}', '\u{094D}', '\u{093C}', '\u{1100}', '\u{AC00}', '\u{11A8}',
            '\u{0600}', '\u{0903}', '❤', '\u{E0067}', '\u{E007F}', '\u{20E3}', '1', '\u{0644}', '\u{0627}'];
        for a in &alphabet { for b in &alphabet { for c in &alphabet {
            samples.push([*a, *b, *c].iter().collect());
        }}}
        for s in samples {
            let cw: Vec<String> = s.chars().map(|c| c.width().map_or("N".into(), |w| w.to_string())).collect();
            let gr: Vec<String> = s.grapheme_indices(true).map(|(i, _)| i.to_string()).collect();
            println!("{}|{}|{}|{}", esc(&s), s.width(), cw.join(","), gr.join(","));
        }
    }
}
