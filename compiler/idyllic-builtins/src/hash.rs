//! The hash of Meadow's `hash` primitive, for the values Idyll hashes with
//! it: `meadow_rt::hash`'s, word for word, so that the reference interpreter
//! and Meadow's runtimes give the same `Int` for the same value -- which a
//! `HashMap`'s order depends on.
//!
//! FxHash's mixing step per word, splitmix64's finalizer at the end, and a
//! tag before each kind of value, so that `0`, `""` and `'\0'` do not collide
//! by construction. Equal numbers hash alike whatever their type, as
//! `meadow_rt::num::hash_into` has them.

use crate::num::Number;

/// What kind of value comes next: `meadow_rt::hash::Tag`'s numbers.
#[derive(Clone, Copy)]
enum Tag {
    Int = 1,
    BigInt = 2,
    Float = 3,
    Bool = 4,
    Char = 5,
    Str = 6,
}

struct Hasher {
    h: u64,
}

impl Hasher {
    fn new() -> Hasher {
        Hasher { h: 0x6a09_e667_f3bc_c908 }
    }

    fn word(&mut self, w: u64) {
        self.h = (self.h.rotate_left(5) ^ w).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    fn tag(&mut self, t: Tag) {
        self.word(t as u64);
    }

    fn bytes(&mut self, b: &[u8]) {
        self.word(b.len() as u64);
        for chunk in b.chunks(8) {
            let mut w = [0u8; 8];
            w[..chunk.len()].copy_from_slice(chunk);
            self.word(u64::from_le_bytes(w));
        }
    }

    fn finish(&self) -> i64 {
        let mut z = self.h;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        (z ^ (z >> 31)) as i64
    }
}

fn int(n: i64) -> i64 {
    let mut h = Hasher::new();
    h.tag(Tag::Int);
    h.word(n as u64);
    h.finish()
}

/// A number's hash: an integer by the number it stands for (a `UInt64` past
/// `Int`'s largest as a `BigInt`, by its two's-complement bytes), a float by
/// its bits, `0.0` and `-0.0` alike and every NaN alike.
pub fn number(n: Number) -> i64 {
    let float = |x: f64| {
        let mut h = Hasher::new();
        h.tag(Tag::Float);
        let bits = if x == 0.0 {
            0
        } else if x.is_nan() {
            f64::NAN.to_bits()
        } else {
            x.to_bits()
        };
        h.word(bits);
        h.finish()
    };
    match n {
        Number::Int(x) => int(x),
        Number::Float(x) => float(x),
        Number::Float32(x) => float(f64::from(x)),
        Number::Word(w, b) => {
            let v = w.value(b);
            match i64::try_from(v) {
                Ok(x) => int(x),
                Err(_) => {
                    let mut h = Hasher::new();
                    h.tag(Tag::BigInt);
                    h.bytes(&signed_bytes_le(v));
                    h.finish()
                }
            }
        }
    }
}

/// `num_bigint::BigInt::to_signed_bytes_le` of a positive `i128`: its bytes,
/// least significant first, as few as hold it with a sign bit clear.
fn signed_bytes_le(v: i128) -> Vec<u8> {
    let mut bytes = v.to_le_bytes().to_vec();
    while bytes.len() > 1 && bytes[bytes.len() - 1] == 0 && bytes[bytes.len() - 2] & 0x80 == 0 {
        bytes.pop();
    }
    bytes
}

pub fn string(s: &str) -> i64 {
    let mut h = Hasher::new();
    h.tag(Tag::Str);
    h.bytes(s.as_bytes());
    h.finish()
}

pub fn char(c: char) -> i64 {
    let mut h = Hasher::new();
    h.tag(Tag::Char);
    h.word(u64::from(c));
    h.finish()
}

pub fn bool(b: bool) -> i64 {
    let mut h = Hasher::new();
    h.tag(Tag::Bool);
    h.word(u64::from(b));
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::num::Width;

    #[test]
    fn equal_numbers_hash_alike_and_kinds_apart() {
        assert_eq!(number(Number::Int(5)), number(Number::Word(Width::U8, 5)));
        assert_eq!(number(Number::Float(0.0)), number(Number::Float(-0.0)));
        assert_ne!(number(Number::Int(0)), string(""));
        assert_ne!(char('a'), number(Number::Int(97)));
        // a `UInt64` past `Int`'s largest is a `BigInt`'s hash
        assert_eq!(signed_bytes_le(1 << 63), vec![0, 0, 0, 0, 0, 0, 0, 0x80, 0]);
    }
}
