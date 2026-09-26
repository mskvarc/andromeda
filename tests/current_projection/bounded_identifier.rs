//! Identifiers of an exact octet length, for probing the identifier bound.
//!
//! The tail is high-entropy text rather than a repeated character, so a stored
//! identifier near the bound is incompressible and exercises the index entry
//! and TOAST paths the way a real long identifier does.

use andromeda_test_fixtures::high_entropy_text::high_entropy_text;

/// An identifier of exactly `length` UTF-8 octets: `prefix` followed by a
/// deterministic base62 tail drawn from `seed`.
pub fn high_entropy_identifier(prefix: &str, length: usize, seed: u64) -> String {
    let tail = length
        .checked_sub(prefix.len())
        .expect("the prefix must fit inside the requested identifier length");
    let identifier = format!("{prefix}{}", high_entropy_text(tail, seed));
    assert_eq!(identifier.len(), length, "the fixture identifier must have the intended octet length");
    identifier
}
