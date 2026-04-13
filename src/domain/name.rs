use static_regular_grammar::RegularGrammar;

/// A validated NGSI-LD short name (ETSI GS CIM 009 v1.9.1, clause 4.6.2).
///
/// A name starts with a Unicode letter and is followed by zero or more
/// Unicode letters, digits, or underscores. Examples: `"temperature"`,
/// `"Building"`, `"speed_limit"`.
///
/// This type validates short names only, not expanded IRIs. For values
/// that may contain IRIs after JSON-LD expansion, use the appropriate
/// IRI type or `String`.
///
/// This is an unsized type. Use [`NameBuf`] for owned values.
#[derive(RegularGrammar, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[grammar(file = "grammar/name.abnf", sized(NameBuf, derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)), serde)]
pub struct Name(str);

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::{Name, NameBuf};

    #[test]
    fn valid_simple_ascii() {
        assert!(Name::new("temperature").is_ok());
    }

    #[test]
    fn valid_uppercase_start() {
        assert!(Name::new("Building").is_ok());
    }

    #[test]
    fn valid_with_underscores() {
        assert!(Name::new("speed_limit").is_ok());
    }

    #[test]
    fn valid_with_digits() {
        assert!(Name::new("sensor42").is_ok());
    }

    #[test]
    fn valid_single_letter() {
        assert!(Name::new("x").is_ok());
    }

    #[test]
    fn valid_unicode_letter_start() {
        assert!(Name::new("M\u{00fc}nchen").is_ok());
    }

    #[test]
    fn valid_all_uppercase() {
        assert!(Name::new("BUILDING").is_ok());
    }

    #[test]
    fn valid_as_str_roundtrip() {
        let name = Name::new("temperature").expect("valid name");
        pretty_assertions::assert_eq!(name.as_str(), "temperature");
    }

    #[test]
    fn invalid_empty() {
        assert!(Name::new("").is_err());
    }

    #[test]
    fn invalid_starts_with_digit() {
        assert!(Name::new("42sensor").is_err());
    }

    #[test]
    fn invalid_starts_with_underscore() {
        assert!(Name::new("_private").is_err());
    }

    #[test]
    fn invalid_contains_hyphen() {
        assert!(Name::new("speed-limit").is_err());
    }

    #[test]
    fn invalid_contains_space() {
        assert!(Name::new("my name").is_err());
    }

    #[test]
    fn invalid_contains_slash() {
        assert!(Name::new("a/b").is_err());
    }

    #[test]
    fn invalid_contains_dot() {
        assert!(Name::new("a.b").is_err());
    }

    #[test]
    fn invalid_contains_colon() {
        assert!(Name::new("urn:thing").is_err());
    }

    #[test]
    fn invalid_iri() {
        assert!(Name::new("https://example.org/speed").is_err());
    }

    #[test]
    fn serde_deserialize_valid() {
        let result: Result<NameBuf, _> = serde_json::from_str("\"temperature\"");
        let name = result.expect("valid name deserializes");
        pretty_assertions::assert_eq!(name.as_str(), "temperature");
    }

    #[test]
    fn serde_deserialize_invalid_rejects() {
        let result: Result<NameBuf, _> = serde_json::from_str("\"42bad\"");
        assert!(result.is_err());
    }

    #[test]
    fn serde_serialize() {
        let name = NameBuf::new("temperature".to_string()).expect("valid name");
        let json = serde_json::to_string(&name).expect("serializes");
        pretty_assertions::assert_eq!(json, "\"temperature\"");
    }
}
