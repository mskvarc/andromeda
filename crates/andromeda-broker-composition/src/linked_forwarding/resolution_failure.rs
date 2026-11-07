//! Reading a forwarding warning as the reason a Linked Entity stayed unresolved.

use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_representation::linked_resolution_failure::{LinkedResolutionFailure, LinkedResolutionFailureKind};
use tracing::warn;

/// The failures a source's `warnings` leave on the Linked Entity `id`, each
/// warning logged as it is read.
pub(super) fn source_failures(id: &Identifier, warnings: Vec<NgsiLdWarning>) -> impl Iterator<Item = LinkedResolutionFailure> {
    warnings.into_iter().map(move |warning| {
        warn!("Linked entity {} could not be fully resolved from a context source: {}", id, warning);
        resolution_failure(id, warning)
    })
}

/// Restate one forwarding warning as the domain-level reason a Linked Entity
/// stayed unresolved.
///
/// The IANA codes clause 6.3.17 uses are a transport concern. What travels back
/// into the traversal is the condition itself, which the HTTP binding turns
/// into a response warning again when it renders — the domain layer in between
/// never learns that response headers exist. The mapping is the same whether
/// the traversal was over current entities or over their history, so it is
/// stated once.
fn resolution_failure(id: &Identifier, warning: NgsiLdWarning) -> LinkedResolutionFailure {
    let (code, text) = warning.into_parts();
    let kind = match code {
        WarnCode::ResponseIsStale => LinkedResolutionFailureKind::StaleData,
        WarnCode::RevalidationFailed => LinkedResolutionFailureKind::UnusableResponse,
        WarnCode::MiscellaneousWarning => LinkedResolutionFailureKind::Unreachable,
        WarnCode::MiscellaneousPersistentWarning => LinkedResolutionFailureKind::Refused,
    };
    LinkedResolutionFailure {
        id: id.clone(), // clone: the failure outlives the borrowed candidate id
        kind,
        detail: text.into(),
    }
}

#[cfg(test)]
mod tests {
    use crate::linked_forwarding::resolution_failure::resolution_failure;
    use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_representation::linked_resolution_failure::LinkedResolutionFailureKind;

    fn uri(value: &str) -> Identifier {
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
            (NgsiLdWarning::new(WarnCode::ResponseIsStale, "cached"), LinkedResolutionFailureKind::StaleData),
            (
                NgsiLdWarning::new(WarnCode::RevalidationFailed, "bad payload"),
                LinkedResolutionFailureKind::UnusableResponse,
            ),
            (
                NgsiLdWarning::new(WarnCode::MiscellaneousWarning, "timeout"),
                LinkedResolutionFailureKind::Unreachable,
            ),
            (
                NgsiLdWarning::new(WarnCode::MiscellaneousPersistentWarning, "HTTP 403"),
                LinkedResolutionFailureKind::Refused,
            ),
        ];

        for (warning, expected) in cases {
            let detail = warning.text().as_str().to_owned();

            let failure = resolution_failure(&id, warning);

            pretty_assertions::assert_eq!(failure.kind, expected);
            pretty_assertions::assert_eq!(failure.detail, detail);
            pretty_assertions::assert_eq!(failure.id, id);
        }
    }
}
