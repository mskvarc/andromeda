//! Reading a forwarding warning as the reason a Linked Entity stayed unresolved.

use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_representation::linked_resolution_failure::{LinkedResolutionFailure, LinkedResolutionFailureKind};

/// Restate one forwarding warning as the domain-level reason a Linked Entity
/// stayed unresolved.
///
/// The IANA codes clause 6.3.17 uses are a transport concern. What travels back
/// into the traversal is the condition itself, which the HTTP binding turns
/// into a response warning again when it renders — the domain layer in between
/// never learns that response headers exist. The mapping is the same whether
/// the traversal was over current entities or over their history, so it is
/// stated once.
pub(crate) fn resolution_failure(id: &Uri, warning: NgsiLdWarning) -> LinkedResolutionFailure {
    let (kind, detail) = match warning {
        NgsiLdWarning::ResponseIsStale(detail) => (LinkedResolutionFailureKind::StaleData, detail),
        NgsiLdWarning::RevalidationFailed(detail) => (LinkedResolutionFailureKind::UnusableResponse, detail),
        NgsiLdWarning::MiscellaneousWarning(detail) => (LinkedResolutionFailureKind::Unreachable, detail),
        NgsiLdWarning::MiscellaneousPersistentWarning(detail) => (LinkedResolutionFailureKind::Refused, detail),
    };
    LinkedResolutionFailure {
        id: id.clone(), // clone: the failure outlives the borrowed candidate id
        kind,
        detail,
    }
}

#[cfg(test)]
mod tests {
    use crate::linked_resolution_failure_mapping::resolution_failure;
    use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_representation::linked_resolution_failure::LinkedResolutionFailureKind;

    fn uri(value: &str) -> Uri {
        value.parse().expect("valid URI")
    }

    /// Every warning a context source can raise while a Linked Entity is being
    /// fetched maps onto exactly one domain-level condition, carrying the
    /// source's own detail text and naming the entity that stayed unresolved.
    /// The mapping is total, so a warning can never be silently swallowed.
    #[test]
    fn each_forwarding_warning_becomes_the_condition_it_describes() {
        let id = uri("urn:ngsi-ld:Building:1");
        let cases = [
            (NgsiLdWarning::ResponseIsStale("cached".to_owned()), LinkedResolutionFailureKind::StaleData),
            (
                NgsiLdWarning::RevalidationFailed("bad payload".to_owned()),
                LinkedResolutionFailureKind::UnusableResponse,
            ),
            (
                NgsiLdWarning::MiscellaneousWarning("timeout".to_owned()),
                LinkedResolutionFailureKind::Unreachable,
            ),
            (
                NgsiLdWarning::MiscellaneousPersistentWarning("HTTP 403".to_owned()),
                LinkedResolutionFailureKind::Refused,
            ),
        ];

        for (warning, expected) in cases {
            let detail = match &warning {
                NgsiLdWarning::ResponseIsStale(detail)
                | NgsiLdWarning::RevalidationFailed(detail)
                | NgsiLdWarning::MiscellaneousWarning(detail)
                | NgsiLdWarning::MiscellaneousPersistentWarning(detail) => detail.clone(), // clone: the expectation outlives the moved warning
            };

            let failure = resolution_failure(&id, warning);

            pretty_assertions::assert_eq!(failure.kind, expected);
            pretty_assertions::assert_eq!(failure.detail, detail);
            pretty_assertions::assert_eq!(failure.id, id);
        }
    }
}
