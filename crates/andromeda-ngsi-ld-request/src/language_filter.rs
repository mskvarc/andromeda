//! Deserialization of the clause-4.15 NGSI-LD Language Filter.

use andromeda_http_headers::language::LanguageFilter;
use serde::{Deserialize, Deserializer, de::Error as DeserializeError};

/// Deserialize an optional language filter from its query-string grammar.
///
/// Clause 4.15 permits comma-separated language ranges, wildcard ranges and
/// RFC quality factors, so a single language-tag value is not sufficient for
/// URL parameters.
pub fn deserialize_optional_language_filter<'de, D>(deserializer: D) -> Result<Option<LanguageFilter>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)?
        .map(|raw| LanguageFilter::parse(&raw).map_err(DeserializeError::custom))
        .transpose()
}

#[cfg(test)]
mod tests {
    use crate::language_filter::deserialize_optional_language_filter;
    use andromeda_http_headers::language::LanguageFilter;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Parameters {
        #[serde(default, deserialize_with = "deserialize_optional_language_filter")]
        lang: Option<LanguageFilter>,
    }

    #[test]
    fn ranked_language_filter_is_deserialized() {
        let parameters: Parameters = serde_urlencoded::from_str("lang=fr-CH%2Cfr%3Bq%3D0.9%2C%2A%3Bq%3D0.5").expect("valid language filter");
        pretty_assertions::assert_eq!(parameters.lang.expect("language filter").preferences().len(), 3);
    }

    #[test]
    fn malformed_language_filter_is_rejected() {
        let result = serde_urlencoded::from_str::<Parameters>("lang=not_a_language");
        assert!(result.is_err());
    }
}
