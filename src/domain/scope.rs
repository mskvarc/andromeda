use static_regular_grammar::RegularGrammar;

/// A validated NGSI-LD scope path (ETSI GS CIM 009 clause 4.18).
///
/// Scope paths are hierarchical, slash-separated labels like `/Madrid/Gardens/ParqueNorte`.
/// Each level starts with a Unicode letter, followed by Unicode letters, digits, or
/// underscores. The special value `urn:ngsi-ld:null` represents a deleted scope.
///
/// This is an unsized type. Use [`ScopeBuf`] for owned values.
#[derive(RegularGrammar, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[grammar(file = "grammar/scope.abnf", sized(ScopeBuf, derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)), serde)]
pub struct Scope(str);

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::{Scope, ScopeBuf};

    #[test]
    fn valid_single_level_with_slash() {
        assert!(Scope::new("/Madrid").is_ok());
    }

    #[test]
    fn valid_single_level_without_slash() {
        assert!(Scope::new("Madrid").is_ok());
    }

    #[test]
    fn valid_multi_level() {
        assert!(Scope::new("/Madrid/Gardens/ParqueNorte").is_ok());
    }

    #[test]
    fn valid_company_hierarchy() {
        assert!(Scope::new("/CompanyA/OrganizationB/UnitC").is_ok());
    }

    #[test]
    fn valid_with_underscores() {
        assert!(Scope::new("/Company_A/Org_B").is_ok());
    }

    #[test]
    fn valid_with_digits() {
        assert!(Scope::new("/Floor1/Room42").is_ok());
    }

    #[test]
    fn valid_null_scope() {
        assert!(Scope::new("urn:ngsi-ld:null").is_ok());
    }

    #[test]
    fn valid_unicode_letters() {
        assert!(Scope::new("/M\u{00fc}nchen/Stra\u{00df}e").is_ok());
    }

    #[test]
    fn valid_as_str_roundtrip() {
        let scope = Scope::new("/A/B").expect("valid scope");
        pretty_assertions::assert_eq!(scope.as_str(), "/A/B");
    }

    #[test]
    fn invalid_empty_string() {
        assert!(Scope::new("").is_err());
    }

    #[test]
    fn invalid_only_slash() {
        assert!(Scope::new("/").is_err());
    }

    #[test]
    fn invalid_trailing_slash() {
        assert!(Scope::new("/A/B/").is_err());
    }

    #[test]
    fn invalid_double_slash() {
        assert!(Scope::new("/A//B").is_err());
    }

    #[test]
    fn invalid_starts_with_digit() {
        assert!(Scope::new("/1Floor").is_err());
    }

    #[test]
    fn invalid_starts_with_underscore() {
        assert!(Scope::new("/_hidden").is_err());
    }

    #[test]
    fn invalid_hyphen() {
        assert!(Scope::new("/A/B-C").is_err());
    }

    #[test]
    fn invalid_space() {
        assert!(Scope::new("/A B").is_err());
    }

    #[test]
    fn invalid_partial_null_urn() {
        assert!(Scope::new("urn:ngsi-ld:nul").is_err());
    }

    #[test]
    fn serde_deserialize_valid() {
        let result: Result<ScopeBuf, _> = serde_json::from_str("\"/Madrid/Gardens\"");
        let scope = result.expect("valid scope deserializes");
        pretty_assertions::assert_eq!(scope.as_str(), "/Madrid/Gardens");
    }

    #[test]
    fn serde_deserialize_invalid_rejects() {
        let result: Result<ScopeBuf, _> = serde_json::from_str("\"//invalid\"");
        assert!(result.is_err());
    }

    #[test]
    fn serde_serialize() {
        let scope = ScopeBuf::new("/Madrid".to_string()).expect("valid scope");
        let json = serde_json::to_string(&scope).expect("serializes");
        pretty_assertions::assert_eq!(json, "\"/Madrid\"");
    }
}
