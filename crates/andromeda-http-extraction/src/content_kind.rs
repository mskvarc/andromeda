//! The JSON media types an NGSI-LD request body may carry (clause 6.3.4).
//!
//! Uses `mediatype` for `Content-Type` validation instead of substring
//! matching, preventing false positives: `application/json-patch+json` is not
//! `application/json`.

use axum::http::{HeaderMap, Method};
use headers::HeaderMapExt;
use mediatype::{MediaTypeBuf, names};

/// Classify the Content-Type into one of the supported NGSI-LD types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ContentKind {
    /// `application/json` -- exact type, no suffix
    Json,
    /// `application/ld+json` -- subtype=ld, suffix=json
    LdJson,
    /// `application/merge-patch+json` -- subtype=merge-patch, suffix=json
    MergePatchJson,
}

/// The representation a request's `Content-Type` announces, when clause 6.3.4
/// admits it under the request's method.
pub(crate) fn request_content_kind(headers: &HeaderMap, method: &Method) -> Option<ContentKind> {
    classify_content_type(&headers.typed_get::<headers::ContentType>()?.to_string(), method)
}

/// Classify a raw content type into a supported JSON representation kind.
///
/// Clause 6.3.4: `application/merge-patch+json` is admitted for HTTP PATCH
/// requests alone, so under any other method it is unsupported.
pub(crate) fn classify_content_type(raw: &str, method: &Method) -> Option<ContentKind> {
    let mt: MediaTypeBuf = raw.parse().ok()?;

    if mt.ty() != names::APPLICATION {
        return None;
    }

    // application/ld+json
    if mt.subty() == names::LD && mt.suffix() == Some(names::JSON) {
        return Some(ContentKind::LdJson);
    }

    // application/merge-patch+json
    if *method == Method::PATCH && mt.subty() == names::MERGE_PATCH && mt.suffix() == Some(names::JSON) {
        return Some(ContentKind::MergePatchJson);
    }

    // application/json -- exact, no suffix (rejects application/json-patch+json etc.)
    if mt.subty() == names::JSON && mt.suffix().is_none() {
        return Some(ContentKind::Json);
    }

    None
}

#[cfg(test)]
mod tests {
    use crate::content_kind::{ContentKind, classify_content_type};
    use axum::http::Method;

    #[test]
    fn the_three_supported_json_media_types_are_told_apart() {
        pretty_assertions::assert_eq!(classify_content_type("application/json", &Method::POST), Some(ContentKind::Json));
        pretty_assertions::assert_eq!(classify_content_type("application/ld+json", &Method::POST), Some(ContentKind::LdJson));
        pretty_assertions::assert_eq!(
            classify_content_type("application/merge-patch+json", &Method::PATCH),
            Some(ContentKind::MergePatchJson)
        );
    }

    #[test]
    fn a_charset_parameter_does_not_change_the_kind() {
        pretty_assertions::assert_eq!(classify_content_type("application/json; charset=utf-8", &Method::PUT), Some(ContentKind::Json));
    }

    #[test]
    fn any_other_suffixed_or_foreign_media_type_is_unsupported() {
        for raw in ["application/json-patch+json", "application/xml", "text/json", "not a media type", ""] {
            pretty_assertions::assert_eq!(classify_content_type(raw, &Method::PATCH), None, "{raw}");
        }
    }

    #[test]
    fn merge_patch_is_unsupported_under_every_method_but_patch() {
        for method in [Method::POST, Method::PUT, Method::GET, Method::DELETE] {
            pretty_assertions::assert_eq!(classify_content_type("application/merge-patch+json", &method), None, "{method}");
        }
    }
}
