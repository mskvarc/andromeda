//! Text and entity identifiers of an exact length, for a test whose subject is the length of a value (a bound in
//! octets, or a reference of thousands of octets, which clause 4.6.2 does not bound) however long the namespace in its
//! prefix is.

use crate::namespace::{Namespace, NamespaceError};
use ngsi_ld::model::EntityId;

/// The character text is filled with.
const FILL: char = 'a';

/// Why a prefix cannot be padded.
#[derive(Debug, thiserror::Error)]
#[error("the prefix is {prefix} octets, longer than the {length} octets asked for")]
pub struct PaddingError {
    /// The octets the prefix takes.
    prefix: usize,
    /// The octets asked for.
    length: usize,
}

/// `length` fill characters.
fn fill(length: usize) -> String {
    FILL.to_string().repeat(length)
}

/// `prefix` filled up to `length` octets.
///
/// # Errors
///
/// Returns [`PaddingError`] when `prefix` alone takes more than `length` octets.
pub fn padded(prefix: &str, length: usize) -> Result<String, PaddingError> {
    let missing = length.checked_sub(prefix.len()).ok_or(PaddingError { prefix: prefix.len(), length })?;
    Ok(format!("{prefix}{}", fill(missing)))
}

/// The URN of `kind` in `namespace` whose suffix after `urn:ngsi-ld:<kind>:<namespace>-` is `length` characters long.
///
/// # Errors
///
/// Returns [`NamespaceError`] when the text is not a valid identifier.
pub fn urn(namespace: &Namespace, kind: &str, length: usize) -> Result<EntityId, NamespaceError> {
    namespace.urn(kind, fill(length))
}

#[cfg(test)]
mod tests {
    use crate::{
        exact_length::{padded, urn},
        namespace::Namespace,
    };
    use googletest::prelude::*;

    #[gtest]
    fn a_prefix_is_padded_to_the_length_in_octets() -> Result<()> {
        let text = padded("urn:ngsi-ld:Building:x-", 128)?;
        expect_that!(text.len(), eq(128));
        verify_that!(text, starts_with("urn:ngsi-ld:Building:x-a"))
    }

    #[gtest]
    fn a_prefix_as_long_as_the_length_is_returned_whole() -> Result<()> {
        verify_that!(padded("abc", 3)?, eq("abc"))
    }

    #[gtest]
    fn a_prefix_longer_than_the_length_is_refused() {
        expect_that!(padded("abcd", 3).is_err(), eq(true));
    }

    #[gtest]
    fn an_identifier_ends_with_a_suffix_of_the_stated_length() -> Result<()> {
        let namespace = Namespace::fresh();
        let id = urn(&namespace, "Stop", 5982)?;
        let prefix = namespace.urn_text("Stop", "");
        expect_that!(id.to_string(), starts_with(prefix.as_str()));
        verify_that!(id.to_string().len(), eq(prefix.len() + 5982))
    }
}
