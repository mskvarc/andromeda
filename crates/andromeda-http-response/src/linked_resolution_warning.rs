//! Turning an unresolved Linked Entity into a response warning.
//!
//! A join that could not reach one of the entities it was asked for still
//! answers (clause 4.5.23 leaves the Relationship as a plain URI), so the only
//! way a client learns that the body is incomplete is the `NGSILD-Warning`
//! header clause 6.3.17 defines. The traversal reports the condition in domain
//! terms — it knows nothing about headers — and this is where that condition
//! becomes an IANA warning code.
//!
//! Both joins report through it: the entity join of clause 4.5.23 and the same
//! clause applied to the temporal API. The condition a traversal names carries
//! no trace of which one it came from, so one mapping answers for both.

use crate::linked_entity_unresolved::LinkedEntityUnresolved;
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_representation::linked_resolution_failure::{LinkedResolutionFailure, LinkedResolutionFailureKind};

/// Append the warnings the unresolved Linked Entities call for (clause
/// 6.3.17), keeping the warnings the forwarded retrieval already produced.
pub fn with_linked_resolution_warnings(mut warnings: Vec<NgsiLdWarning>, failures: &[LinkedResolutionFailure]) -> Vec<NgsiLdWarning> {
    warnings.extend(failures.iter().map(linked_resolution_warning));
    warnings
}

/// The clause 6.3.17 warning one unresolved Linked Entity is reported as.
fn linked_resolution_warning(failure: &LinkedResolutionFailure) -> NgsiLdWarning {
    let detail = LinkedEntityUnresolved(failure).to_string();
    match failure.kind {
        LinkedResolutionFailureKind::StaleData => NgsiLdWarning::ResponseIsStale(detail),
        LinkedResolutionFailureKind::UnusableResponse => NgsiLdWarning::RevalidationFailed(detail),
        LinkedResolutionFailureKind::Unreachable => NgsiLdWarning::MiscellaneousWarning(detail),
        LinkedResolutionFailureKind::Refused => NgsiLdWarning::MiscellaneousPersistentWarning(detail),
    }
}

#[cfg(test)]
mod tests {
    use crate::linked_resolution_warning::with_linked_resolution_warnings;
    use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_representation::linked_resolution_failure::{LinkedResolutionFailure, LinkedResolutionFailureKind};

    fn failure(kind: LinkedResolutionFailureKind) -> LinkedResolutionFailure {
        LinkedResolutionFailure {
            id: "urn:ngsi-ld:Building:1".parse::<Identifier>().expect("valid URI"),
            kind,
            detail: "the context source did not respond".to_owned(),
        }
    }

    /// Each condition picks the IANA code clause 6.3.17 reserves for it, and
    /// the entity that stayed unresolved is named in the warning text so a
    /// client can tell which Relationship stayed a plain URI.
    #[test]
    fn each_condition_maps_to_its_warning_code() {
        let expected = [
            (
                LinkedResolutionFailureKind::StaleData,
                NgsiLdWarning::ResponseIsStale("linked entity urn:ngsi-ld:Building:1 could not be resolved: the context source did not respond".to_owned()),
            ),
            (
                LinkedResolutionFailureKind::UnusableResponse,
                NgsiLdWarning::RevalidationFailed("linked entity urn:ngsi-ld:Building:1 could not be resolved: the context source did not respond".to_owned()),
            ),
            (
                LinkedResolutionFailureKind::Unreachable,
                NgsiLdWarning::MiscellaneousWarning(
                    "linked entity urn:ngsi-ld:Building:1 could not be resolved: the context source did not respond".to_owned(),
                ),
            ),
            (
                LinkedResolutionFailureKind::Refused,
                NgsiLdWarning::MiscellaneousPersistentWarning(
                    "linked entity urn:ngsi-ld:Building:1 could not be resolved: the context source did not respond".to_owned(),
                ),
            ),
        ];

        for (kind, warning) in expected {
            pretty_assertions::assert_eq!(with_linked_resolution_warnings(Vec::new(), &[failure(kind)]), vec![warning]);
        }
    }

    /// The warnings the forwarded retrieval already produced keep their place
    /// ahead of the join's, so a response reports what happened in the order it
    /// happened.
    #[test]
    fn the_join_warnings_follow_the_forwarding_warnings() {
        let existing = NgsiLdWarning::MiscellaneousWarning("Context source did not respond".to_owned());

        let warnings = with_linked_resolution_warnings(vec![existing.clone()], &[failure(LinkedResolutionFailureKind::Refused)]); // clone: the expectation keeps its own copy

        pretty_assertions::assert_eq!(warnings.len(), 2);
        pretty_assertions::assert_eq!(warnings.first(), Some(&existing));
    }

    /// A join that resolved everything adds nothing.
    #[test]
    fn a_complete_join_adds_no_warning() {
        assert_eq!(with_linked_resolution_warnings(Vec::new(), &[]), [] as [NgsiLdWarning; 0]);
    }
}
