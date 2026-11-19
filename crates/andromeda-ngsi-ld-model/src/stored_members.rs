//! Serde support for the lists of a stored subscription whose members are read back in the expanded stage.

use serde::{Deserialize, Deserializer, de::IgnoredAny};

/// A list member that is either read in its stage or left out.
#[derive(Deserialize)]
#[serde(untagged)]
enum StoredMember<T> {
    /// A member that is valid in its stage.
    Readable(T),
    /// A member that is not, skipped without reading it.
    Unreadable(IgnoredAny),
}

/// Deserialize a list, leaving out each member that is not valid in its stage.
///
/// A stored name that is no absolute IRI names no attribute of an expanded entity, so it selects nothing and leaving it out keeps what the rest of the list selects.
///
/// # Errors
///
/// Returns the deserializer's own error when the member is not a list.
pub fn deserialize_skipping_unreadable<'de, D, T>(deserializer: D) -> Result<Option<Vec<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<StoredMember<T>>>::deserialize(deserializer)?.map(|members| {
        members
            .into_iter()
            .filter_map(|member| match member {
                StoredMember::Readable(readable) => Some(readable),
                StoredMember::Unreadable(_skipped) => None,
            })
            .collect()
    }))
}

#[cfg(test)]
mod tests {
    use crate::stored_members::deserialize_skipping_unreadable;
    use andromeda_ngsi_ld_lexicon::attribute_iri::AttributeIri;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Debug, Deserialize)]
    struct Carrier {
        #[serde(default, deserialize_with = "deserialize_skipping_unreadable")]
        names: Option<Vec<AttributeIri>>,
    }

    #[test]
    fn a_member_that_is_no_absolute_iri_is_left_out_and_the_rest_kept() {
        let carrier: Carrier = serde_json::from_value(json!({"names": ["https://example.com/a", "bare", "https://example.com/b"]})).expect("readable");
        pretty_assertions::assert_eq!(carrier.names.map(|names| names.len()), Some(2));
    }

    #[test]
    fn an_absent_list_stays_absent_and_a_non_list_is_refused() {
        assert!(serde_json::from_value::<Carrier>(json!({})).expect("absent").names.is_none());
        assert!(serde_json::from_value::<Carrier>(json!({"names": "https://example.com/a"})).is_err());
    }
}
