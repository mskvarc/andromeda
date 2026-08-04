use static_regular_grammar::RegularGrammar;

/// A validated NGSI-LD attribute path (ETSI GS CIM 009 v1.9.1, clauses
/// 4.5.24 and 4.9).
///
/// A dotted sequence of [`crate::name::Name`] segments, optionally
/// followed by a bracketed modifier — a language tag (for
/// `LanguageProperty`) or a `datasetId` URI. Used by the `attrs=` query
/// parameter, the `q=` filter language, and merge-patch target paths.
///
/// This is an unsized type. Use [`AttributePathBuf`] for owned values.
#[derive(RegularGrammar, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[grammar(
    file = "grammar/.generated/attribute-path.abnf",
    sized(AttributePathBuf, derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)),
    serde
)]
pub struct AttributePath(str);

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::AttributePath;

    #[test]
    fn valid_single_segment() {
        assert!(AttributePath::new("speed").is_ok());
    }

    #[test]
    fn valid_nested_segments() {
        assert!(AttributePath::new("speed.accuracy").is_ok());
    }

    #[test]
    fn valid_with_langtag_modifier() {
        assert!(AttributePath::new("name[en]").is_ok());
    }

    #[test]
    fn valid_nested_with_dataset_uri() {
        assert!(AttributePath::new("address.street[urn:ds:1]").is_ok());
    }

    #[test]
    fn valid_prefixed_name_segment() {
        assert!(AttributePath::new("ngsild:speed").is_ok());
    }

    #[test]
    fn invalid_leading_dot() {
        assert!(AttributePath::new(".speed").is_err());
    }

    #[test]
    fn invalid_trailing_dot() {
        assert!(AttributePath::new("speed.").is_err());
    }

    #[test]
    fn invalid_empty_modifier() {
        assert!(AttributePath::new("speed[]").is_err());
    }

    #[test]
    fn invalid_dot_before_modifier() {
        assert!(AttributePath::new("speed.[en]").is_err());
    }

    #[test]
    fn invalid_empty() {
        assert!(AttributePath::new("").is_err());
    }
}
