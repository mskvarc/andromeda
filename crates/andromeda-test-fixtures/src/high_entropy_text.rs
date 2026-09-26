//! Deterministic high-entropy text for tests that store values near a byte
//! bound.
//!
//! A repeated character compresses to almost nothing and yields a handful of
//! trigrams, so it exercises neither TOAST nor a trigram index the way a real
//! long identifier or description does. This text is drawn uniformly from the
//! base62 alphabet by a seeded generator, so it is incompressible, rich in
//! distinct trigrams, ASCII -- one octet per character -- and identical on
//! every run with the same seed.

use rand::{RngExt, SeedableRng, rngs::StdRng};

/// The base62 alphabet the text is drawn from.
const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// `len` characters of base62 text, the same for the same `seed`.
#[must_use]
pub fn high_entropy_text(len: usize, seed: u64) -> String {
    let mut generator = StdRng::seed_from_u64(seed);
    (0..len).map(|_| char::from(BASE62[generator.random_range(0..BASE62.len())])).collect()
}

#[cfg(test)]
mod tests {
    use crate::high_entropy_text::high_entropy_text;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_text_has_the_requested_length_in_octets() {
        assert_eq!(high_entropy_text(1024, 7).len(), 1024);
    }

    #[test]
    fn the_same_seed_yields_the_same_text_and_another_seed_another() {
        assert_eq!(high_entropy_text(64, 1), high_entropy_text(64, 1));
        assert_ne!(high_entropy_text(64, 1), high_entropy_text(64, 2));
    }

    #[test]
    fn the_text_is_base62() {
        assert!(high_entropy_text(512, 3).chars().all(|character| character.is_ascii_alphanumeric()));
    }
}
