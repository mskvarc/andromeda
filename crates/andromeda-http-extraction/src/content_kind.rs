//! The JSON media types an NGSI-LD request body may carry (clause 6.3.4).
//!
//! Uses `mediatype` for `Content-Type` validation instead of substring
//! matching, preventing false positives: `application/json-patch+json` is not
//! `application/json`.

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

/// Classify a raw content type into a supported JSON representation kind.
pub(crate) fn classify_content_type(raw: &str) -> Option<ContentKind> {
    let mt: MediaTypeBuf = raw.parse().ok()?;

    if mt.ty() != names::APPLICATION {
        return None;
    }

    // application/ld+json
    if mt.subty() == names::LD && mt.suffix() == Some(names::JSON) {
        return Some(ContentKind::LdJson);
    }

    // application/merge-patch+json
    if mt.subty() == names::MERGE_PATCH && mt.suffix() == Some(names::JSON) {
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

    #[test]
    fn the_three_supported_json_media_types_are_told_apart() {
        pretty_assertions::assert_eq!(classify_content_type("application/json"), Some(ContentKind::Json));
        pretty_assertions::assert_eq!(classify_content_type("application/ld+json"), Some(ContentKind::LdJson));
        pretty_assertions::assert_eq!(classify_content_type("application/merge-patch+json"), Some(ContentKind::MergePatchJson));
    }

    #[test]
    fn a_charset_parameter_does_not_change_the_kind() {
        pretty_assertions::assert_eq!(classify_content_type("application/json; charset=utf-8"), Some(ContentKind::Json));
    }

    #[test]
    fn any_other_suffixed_or_foreign_media_type_is_unsupported() {
        for raw in ["application/json-patch+json", "application/xml", "text/json", "not a media type", ""] {
            pretty_assertions::assert_eq!(classify_content_type(raw), None, "{raw}");
        }
    }
}
