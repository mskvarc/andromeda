//! Turning a matching entity this broker holds in a form it no longer reads
//! into a response warning.
//!
//! Such an entity is left out of the page rather than failing it, so the
//! client learns that the body is incomplete only from a warning. Clause
//! 6.3.17 gives the `NGSILD-Warning` header that role for a page a context
//! source left incomplete, and this is the same condition on the broker's own
//! store: an entity that matched is missing from the body. The warn-code is
//! 299, the IANA code for a warning that persists (RFC 7234 section 5.5.7),
//! because the entity reads no better on the next request.

use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use thiserror::Error;

/// A matching entity left out of a page because its stored rows no longer read
/// back.
#[derive(Debug, Error)]
#[error("the stored entity {0} does not read back and is left out of the response")]
struct UnreadableEntityLeftOut<'a>(&'a Identifier);

/// Append one warning for each entity in `unreadable` that matched the query
/// but is left out of the page, keeping the warnings already gathered.
#[must_use]
pub fn with_unreadable_entity_warnings(mut warnings: Vec<NgsiLdWarning>, unreadable: &[Identifier]) -> Vec<NgsiLdWarning> {
    warnings.extend(
        unreadable
            .iter()
            .map(|id| NgsiLdWarning::new(WarnCode::MiscellaneousPersistentWarning, UnreadableEntityLeftOut(id))),
    );
    warnings
}

#[cfg(test)]
mod tests {
    use crate::unreadable_entity_warning::with_unreadable_entity_warnings;
    use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use pretty_assertions::assert_eq;

    #[test]
    fn each_unreadable_entity_is_named_by_a_persistent_warning_after_those_already_gathered() {
        let earlier = NgsiLdWarning::new(WarnCode::MiscellaneousWarning, "context source s1 did not respond");
        let unreadable: Identifier = "urn:ngsi-ld:Building:7".parse().expect("valid URI");
        let warnings = with_unreadable_entity_warnings(vec![earlier.clone()], &[unreadable]); // clone: the expectation keeps the earlier warning
        assert_eq!(
            warnings,
            vec![
                earlier,
                NgsiLdWarning::new(
                    WarnCode::MiscellaneousPersistentWarning,
                    "the stored entity urn:ngsi-ld:Building:7 does not read back and is left out of the response"
                ),
            ]
        );
    }

    #[test]
    fn a_page_without_unreadable_entities_keeps_its_warnings_unchanged() {
        assert_eq!(with_unreadable_entity_warnings(Vec::new(), &[]), Vec::<NgsiLdWarning>::new());
    }
}
