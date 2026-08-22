//! Deserialization of a single BCP 47 language tag query parameter.

use langtag::LangTagBuf;
use serde::{Deserialize, Deserializer, de::Error as DeserializeError};

/// Deserialize an optional BCP 47 language tag.
///
/// Table 6.5.3.4-1 `lang` names "the natural language of data held in the
/// request": one tag, not the ranked filter of clause 4.15 a query carries.
pub fn deserialize_optional_language_tag<'de, D>(deserializer: D) -> Result<Option<LangTagBuf>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)?
        .map(|raw| LangTagBuf::new(raw).map_err(|error| DeserializeError::custom(format!("invalid BCP47 language tag: {error}"))))
        .transpose()
}

#[cfg(test)]
mod tests {
    use crate::language_tag::deserialize_optional_language_tag;
    use langtag::LangTagBuf;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Parameters {
        #[serde(default, deserialize_with = "deserialize_optional_language_tag")]
        lang: Option<LangTagBuf>,
    }

    #[test]
    fn a_valid_tag_is_read_and_absence_is_none() {
        let parameters: Parameters = serde_urlencoded::from_str("lang=fr-CH").expect("valid tag");
        pretty_assertions::assert_eq!(parameters.lang.as_ref().map(ToString::to_string).as_deref(), Some("fr-CH"));
        let absent: Parameters = serde_urlencoded::from_str("").expect("parses");
        assert!(absent.lang.is_none());
    }

    #[test]
    fn a_malformed_tag_is_rejected() {
        assert!(serde_urlencoded::from_str::<Parameters>("lang=not_a_language").is_err());
    }
}
