//! The storage bound on an NGSI-LD identifier or name.

use thiserror::Error;

/// The greatest length, in UTF-8 octets, an identifier or an NGSI-LD name may have.
///
/// Clause 4.6.2 sets no length on a URI, and neither does the compacted-name
/// grammar every attribute name, entity type name and reference shares, so this
/// is Andromeda's own bound. It exists because every such value is a key in a
/// B-tree index, and PostgreSQL refuses a B-tree entry larger than 2704 bytes
/// with SQLSTATE 54000 ("index row size exceeds btree maximum"), a failure
/// that would otherwise surface as an internal error on a request whose only
/// fault is its length. 1024 octets is chosen so that two identifiers fit one
/// key together, which the entity-map entries, the forwarded subscriptions and
/// the verbatim instance claims all index on. Refusing the value here answers
/// the request as bad input before anything reaches the store.
///
/// The count is in octets rather than characters because that is what the
/// index entry occupies: a non-ASCII IRI (clause 4.6.2 NOTE 2) reaches the
/// bound in fewer characters than an ASCII one.
pub const IDENTIFIER_BYTE_LIMIT: usize = 1024;

/// An identifier or name longer than [`IDENTIFIER_BYTE_LIMIT`].
#[derive(Clone, Copy, Debug, Eq, Error, Hash, PartialEq)]
#[error("an identifier of {length} bytes exceeds the {IDENTIFIER_BYTE_LIMIT} bytes an identifier may occupy")]
pub struct IdentifierTooLong {
    /// The length of the refused value, in UTF-8 octets.
    pub length: usize,
}

impl IdentifierTooLong {
    /// Check `value` against [`IDENTIFIER_BYTE_LIMIT`].
    ///
    /// # Errors
    ///
    /// Returns [`IdentifierTooLong`] carrying the value's length when it is
    /// longer than the limit.
    pub const fn check(value: &str) -> Result<(), IdentifierTooLong> {
        let length = value.len();
        if length > IDENTIFIER_BYTE_LIMIT {
            return Err(IdentifierTooLong { length });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::identifier_length::{IDENTIFIER_BYTE_LIMIT, IdentifierTooLong};

    #[test]
    fn the_identifier_limit_is_1024_octets() {
        pretty_assertions::assert_eq!(IDENTIFIER_BYTE_LIMIT, 1024);
    }

    #[test]
    fn the_check_accepts_the_limit_and_refuses_one_byte_more() {
        let at_limit = "a".repeat(IDENTIFIER_BYTE_LIMIT);
        pretty_assertions::assert_eq!(IdentifierTooLong::check(&at_limit), Ok(()));

        let past_limit = "a".repeat(IDENTIFIER_BYTE_LIMIT + 1);
        let refused = IdentifierTooLong::check(&past_limit);
        pretty_assertions::assert_eq!(refused, Err(IdentifierTooLong { length: 1025 }));
        let message = refused.err().map(|error| error.to_string()).unwrap_or_default();
        assert!(message.contains("1025 bytes"), "{message}");
    }
}
