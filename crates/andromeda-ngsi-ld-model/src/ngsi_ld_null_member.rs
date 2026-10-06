//! Update-fragment members whose data type is a URI, where the NGSI-LD Null is
//! the URI `urn:ngsi-ld:null` (clause 4.5.0).
//!
//! Clause 4.5.0 replaces `null` by `urn:ngsi-ld:null` "in all places where URI
//! strings are valid JSON values", and by "an array with one single NGSI-LD
//! Null" where an array is required, because JSON-LD drops a JSON `null` when
//! it expands. Clause 5.5.8 gives that value the meaning "the target member is
//! deleted". The members read here take URI strings (`jsonldContext`,
//! `datasetId`, the attribute names of `watchedAttributes`), so both encodings
//! of the Null clear them; any other string is read as the member's own type.
//! A member whose data type is free text or a query string is not such a place,
//! and a string that spells the URN is its value.
//!
//! The two encodings are told apart, because clause 5.2.1 refuses an NGSI-LD
//! Null outside the update clauses 5.5.8 and 5.5.12 while JSON-LD drops a JSON
//! `null` from a create: `null` reads as [`Clearable::Cleared`] and the URI as
//! [`Clearable::NgsiLdNull`], which an update treats alike and a create does
//! not.

use crate::clearable::Clearable;
use andromeda_ngsi_ld_entity::ngsi_ld_null::is_ngsi_ld_null;
use serde::{
    Deserialize,
    Deserializer,
    de::{DeserializeOwned, IntoDeserializer},
};

/// Deserialize a member that is a single URI: `null` clears it, the NGSI-LD Null
/// is read as such, any other string is read as the `T` its data type names.
///
/// # Errors
///
/// Returns the deserializer's own error when the member is neither `null` nor
/// a string, and `T`'s refusal of the string otherwise.
pub fn deserialize_uri<'de, D, T>(deserializer: D) -> Result<Clearable<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    match Option::<String>::deserialize(deserializer)? {
        Some(text) if is_ngsi_ld_null(&text) => Ok(Clearable::NgsiLdNull),
        Some(text) => T::deserialize(text.into_deserializer()).map(Clearable::Set),
        None => Ok(Clearable::Cleared),
    }
}

/// Deserialize a member that is an array of URIs: `null` clears it, an array of
/// one NGSI-LD Null is read as such, any other array is read element by element
/// as `T`.
///
/// # Errors
///
/// Returns the deserializer's own error when the member is neither `null` nor
/// an array of strings, and `T`'s refusal of the first element it does not
/// admit otherwise.
pub fn deserialize_uri_array<'de, D, T>(deserializer: D) -> Result<Clearable<Vec<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let Some(texts) = Option::<Vec<String>>::deserialize(deserializer)? else {
        return Ok(Clearable::Cleared);
    };
    if matches!(texts.as_slice(), [only] if is_ngsi_ld_null(only)) {
        return Ok(Clearable::NgsiLdNull);
    }
    texts
        .into_iter()
        .map(|text| T::deserialize(text.into_deserializer()))
        .collect::<Result<Vec<T>, D::Error>>()
        .map(Clearable::Set)
}

#[cfg(test)]
mod tests {
    use crate::{clearable::Clearable, ngsi_ld_null_member};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use iri_rs::IriBuf;
    use serde::Deserialize;
    use serde_json::json;

    /// A carrier standing in for an update fragment.
    #[derive(Debug, Deserialize)]
    struct Carrier {
        #[serde(default, deserialize_with = "ngsi_ld_null_member::deserialize_uri")]
        context: Clearable<IriBuf>,
        #[serde(default, deserialize_with = "ngsi_ld_null_member::deserialize_uri_array")]
        names: Clearable<Vec<AttributeName>>,
    }

    /// Clause 4.5.0: both `null` and `urn:ngsi-ld:null` are the Null of a URI
    /// member and are told apart (clause 5.2.1), and an absent member is
    /// neither.
    #[test]
    fn a_uri_member_tells_json_null_from_the_ngsi_ld_null() {
        let absent: Carrier = serde_json::from_value(json!({})).expect("absent");
        assert!(absent.context.is_absent() && absent.names.is_absent());
        let cleared: Carrier = serde_json::from_value(json!({"context": null})).expect("a Null");
        assert!(matches!(cleared.context, Clearable::Cleared));
        let urn: Carrier = serde_json::from_value(json!({"context": "urn:ngsi-ld:null"})).expect("a Null");
        assert!(matches!(urn.context, Clearable::NgsiLdNull));
        let set: Carrier = serde_json::from_value(json!({"context": "https://example.com/context.jsonld"})).expect("a URI");
        pretty_assertions::assert_eq!(set.context.as_set().map(IriBuf::as_str), Some("https://example.com/context.jsonld"));
    }

    /// Clause 4.5.0: an array member is cleared by `null` or by an array with
    /// one single NGSI-LD Null, and the Null beside another element is not one.
    #[test]
    fn an_array_member_is_cleared_by_json_null_and_by_an_array_of_one_ngsi_ld_null() {
        let cleared: Carrier = serde_json::from_value(json!({"names": null})).expect("a Null");
        assert!(matches!(cleared.names, Clearable::Cleared));
        let urn: Carrier = serde_json::from_value(json!({"names": ["urn:ngsi-ld:null"]})).expect("a Null");
        assert!(matches!(urn.names, Clearable::NgsiLdNull));
        let set: Carrier = serde_json::from_value(json!({"names": ["speed", "urn:ngsi-ld:null"]})).expect("two names");
        pretty_assertions::assert_eq!(set.names.as_set().map(Vec::len), Some(2));
    }

    #[test]
    fn a_member_of_the_wrong_shape_is_refused() {
        assert!(serde_json::from_value::<Carrier>(json!({"context": 5})).is_err());
        assert!(serde_json::from_value::<Carrier>(json!({"context": "not a uri"})).is_err());
        assert!(serde_json::from_value::<Carrier>(json!({"names": "speed"})).is_err());
    }
}
