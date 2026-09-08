//! Where the `@context` goes in a `GeoJSON` response (clause 6.3.6).

use strum::{Display, EnumString};

/// Whether the `@context` appears in a `GeoJSON` body or in a `Link` header.
///
/// The rendering is the token as it appears after `body=` in a `Prefer` header;
/// RFC 7240 makes preference tokens case-insensitive, which `FromStr` honours.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Display, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum GeoJsonBodyPreference {
    /// `body=ld+json`, or absent: `@context` included in the response body.
    #[default]
    #[strum(serialize = "ld+json")]
    LdJson,
    /// `body=json`: `@context` in the `Link` header, omitted from the body.
    #[strum(serialize = "json")]
    Json,
}

#[cfg(test)]
mod tests {
    use crate::geo_json_body::GeoJsonBodyPreference;
    use std::str::FromStr;

    #[test]
    fn the_default_places_the_context_in_the_body() {
        pretty_assertions::assert_eq!(GeoJsonBodyPreference::default(), GeoJsonBodyPreference::LdJson);
    }

    #[test]
    fn each_preference_renders_and_parses_back() {
        for variant in [GeoJsonBodyPreference::LdJson, GeoJsonBodyPreference::Json] {
            pretty_assertions::assert_eq!(GeoJsonBodyPreference::from_str(&variant.to_string()), Ok(variant));
        }
    }

    #[test]
    fn a_preference_token_parses_regardless_of_case() {
        pretty_assertions::assert_eq!(GeoJsonBodyPreference::from_str("LD+JSON"), Ok(GeoJsonBodyPreference::LdJson));
        pretty_assertions::assert_eq!(GeoJsonBodyPreference::from_str("Json"), Ok(GeoJsonBodyPreference::Json));
    }

    #[test]
    fn an_unknown_token_does_not_parse() {
        assert!(GeoJsonBodyPreference::from_str("xml").is_err());
    }
}
