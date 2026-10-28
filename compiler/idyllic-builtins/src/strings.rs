//! What the string natives do, for the back ends written in Rust (the AxCut
//! machine and the reference interpreter) to share; the C runtime and
//! Meadow's runtimes do the same.
//!
//! A string is UTF-8. Its bytes are what `stringByteLength`, `stringByteAt`,
//! `stringSlice` and `stringIndexOf` count, in constant time; its characters
//! are `Char`s, Unicode scalar values, which `stringToChars` and
//! `charsToString` go to and from, and `charCode` and `charFromCode` between
//! a character and its code point.

/// The byte at `i`, if there is one.
pub fn byte_at(s: &str, i: i64) -> Option<i64> {
    usize::try_from(i).ok().and_then(|i| s.as_bytes().get(i)).map(|b| i64::from(*b))
}

/// The bytes `[from, to)`, clamped to the string; a character cut in two is
/// the replacement character.
pub fn slice(s: &str, from: i64, to: i64) -> String {
    let n = s.len() as i64;
    let lo = from.clamp(0, n);
    let hi = to.clamp(lo, n);
    String::from_utf8_lossy(&s.as_bytes()[lo as usize..hi as usize]).into_owned()
}

/// `-1`, `0` or `1` as `a` sorts before, with or after `b`, byte by byte.
pub fn compare(a: &str, b: &str) -> i64 {
    match a.as_bytes().cmp(b.as_bytes()) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// Where `needle` is first found in `hay` at or after the byte `from` (`0`
/// if `from` is negative); `-1` if nowhere. An empty needle is found where
/// it is looked for, if that is in the string.
pub fn index_of(hay: &str, needle: &str, from: i64) -> i64 {
    let (hay, needle) = (hay.as_bytes(), needle.as_bytes());
    let start = from.max(0) as usize;
    if start > hay.len() {
        return -1;
    }
    if needle.is_empty() {
        return start as i64;
    }
    hay[start..].windows(needle.len()).position(|w| w == needle).map_or(-1, |at| (start + at) as i64)
}

/// The character of a code point, if it is one (a Unicode scalar value).
pub fn from_code(code: i64) -> Option<char> {
    u32::try_from(code).ok().and_then(char::from_u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_and_code_points() {
        assert_eq!(byte_at("aé", 1), Some(0xc3));
        assert_eq!(byte_at("a", 1), None);
        assert_eq!(slice("héllo", 0, 2), "h\u{fffd}");
        assert_eq!(slice("hello", -3, 99), "hello");
        assert_eq!(slice("hello", 3, 1), "");
        assert_eq!(compare("ab", "b"), -1);
        assert_eq!(compare("b", "b"), 0);
        assert_eq!(compare("ba", "b"), 1);
        assert_eq!(index_of("a,b,c", ",", 0), 1);
        assert_eq!(index_of("a,b,c", ",", 2), 3);
        assert_eq!(index_of("a,b,c", ";", 0), -1);
        assert_eq!(index_of("abc", "", 3), 3);
        assert_eq!(index_of("abc", "", 4), -1);
        assert_eq!(from_code(233), Some('é'));
        assert_eq!(from_code(0xd800), None);
        assert_eq!(from_code(-1), None);
    }
}

/// Bytes as hex, two lower-case digits a byte: Meadow's `bytesToHex`.
pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Hex digits, two a byte and either case, as bytes: Meadow's
/// `bytesFromHex`; `None` for an odd count or anything that is no digit.
pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    let bytes = s.as_bytes();
    if bytes.len() % 2 != 0 {
        return None;
    }
    bytes
        .chunks_exact(2)
        .map(|pair| {
            let digit = |b: u8| (b as char).to_digit(16);
            Some((digit(pair[0])? << 4 | digit(pair[1])?) as u8)
        })
        .collect()
}
