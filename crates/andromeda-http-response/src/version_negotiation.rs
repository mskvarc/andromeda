//! The status and header a `Prefer: ngsi-ld=<version>` request earns.
//!
//! Every NGSI-LD response wrapper answers the same two questions once its
//! payload is rendered: clause 6.3.6 makes `Preference-Applied:
//! ngsi-ld=<conformant-version>` mandatory whenever the preference was
//! expressed, and the operation tables (for instance Table 6.34.3.1-1) turn the
//! status into 203 Non-Authoritative Information when the body was amended
//! under clause 4.3.6.8. Deciding both in one place keeps the single,
//! collection and batch wrappers from drifting apart.

use andromeda_http_headers::preference_applied::PreferenceApplied;
use andromeda_ngsi_ld_render::version::VersionAdaptation;
use axum::http::{HeaderMap, StatusCode};
use headers::HeaderMapExt;

/// Apply the outcome of version negotiation to a response being assembled.
///
/// Inserts `Preference-Applied` when the rendering echoes a version and
/// answers the status the response carries: 203 when the payload was amended,
/// otherwise `status` as the handler chose it.
pub fn apply_version_negotiation(status: StatusCode, adaptation: VersionAdaptation, headers: &mut HeaderMap) -> StatusCode {
    if let Some(version) = adaptation.applied_version() {
        headers.typed_insert(PreferenceApplied::version(version));
    }
    if adaptation.is_amended() {
        StatusCode::NON_AUTHORITATIVE_INFORMATION
    } else {
        status
    }
}

#[cfg(test)]
mod tests {
    use crate::version_negotiation::apply_version_negotiation;
    use andromeda_http_headers::preference_applied::PreferenceApplied;
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
    use andromeda_ngsi_ld_render::version::VersionAdaptation;
    use axum::http::{HeaderMap, StatusCode};
    use headers::HeaderMapExt;

    #[test]
    fn an_amended_payload_answers_203_and_echoes_the_amended_version() {
        let mut headers = HeaderMap::new();
        let status = apply_version_negotiation(StatusCode::OK, VersionAdaptation::Applied { version: SpecVersion::V1_8 }, &mut headers);
        pretty_assertions::assert_eq!(status, StatusCode::NON_AUTHORITATIVE_INFORMATION);
        pretty_assertions::assert_eq!(headers.typed_get::<PreferenceApplied>(), Some(PreferenceApplied::version(SpecVersion::V1_8)));
    }

    #[test]
    fn a_created_resource_amended_to_an_older_version_is_still_203() {
        // Table 6.34.3.1-1: the 203 row replaces the 201 row, not only the 200 one.
        let mut headers = HeaderMap::new();
        let status = apply_version_negotiation(StatusCode::CREATED, VersionAdaptation::Applied { version: SpecVersion::V1_4 }, &mut headers);
        pretty_assertions::assert_eq!(status, StatusCode::NON_AUTHORITATIVE_INFORMATION);
    }

    #[test]
    fn a_conformant_request_keeps_its_status_and_echoes_the_conformant_version() {
        // Clause 6.3.6: the echo is unconditional once the preference was
        // expressed, even when the payload needed no amendment.
        let mut headers = HeaderMap::new();
        let status = apply_version_negotiation(StatusCode::OK, VersionAdaptation::Unchanged, &mut headers);
        pretty_assertions::assert_eq!(status, StatusCode::OK);
        pretty_assertions::assert_eq!(
            headers.typed_get::<PreferenceApplied>(),
            Some(PreferenceApplied::version(SpecVersion::CONFORMANT))
        );
    }

    #[test]
    fn no_preference_leaves_both_status_and_headers_alone() {
        let mut headers = HeaderMap::new();
        let status = apply_version_negotiation(StatusCode::CREATED, VersionAdaptation::None, &mut headers);
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
        assert!(headers.typed_get::<PreferenceApplied>().is_none());
    }
}
