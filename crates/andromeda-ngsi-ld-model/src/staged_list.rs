//! A stored list of a subscription read in the stage the broker stores it in, or told apart as written.

use serde::{Deserialize, Deserializer};

/// A stored list whose elements are all valid in the expanded stage `E`, or else all valid in the written stage `W`.
///
/// The broker stores a name only after expanding it, so a stored name is an absolute IRI. A list that reads only as `W` holds a name that is no absolute IRI: it was not written by the broker, and it selects nothing in an expanded store. A list that reads as neither holds an element that is no name at all.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum StagedList<E, W> {
    /// Every element is valid in the expanded stage.
    Expanded(Vec<E>),
    /// Every element is valid in the written stage, and at least one is no expanded name.
    Written(Vec<W>),
}

/// Deserialize a list of the expanded stage `E`, `W` being the same elements in the written stage: a list that reads only as `W` is held as absent, for the reader to list as unreadable through [`StagedList`].
///
/// Dropping the offending elements instead would narrow the list, and a list that is emptied or absent selects every attribute, so a list is never partly kept.
///
/// # Errors
///
/// Returns the deserializer's own error when the member is no list, or holds an element that is no `W` either.
pub fn deserialize_expanded_list<'de, W, D, E>(deserializer: D) -> Result<Option<Vec<E>>, D::Error>
where
    D: Deserializer<'de>,
    E: Deserialize<'de>,
    W: Deserialize<'de>,
{
    Ok(match Option::<StagedList<E, W>>::deserialize(deserializer)? {
        Some(StagedList::Expanded(elements)) => Some(elements),
        Some(StagedList::Written(_)) | None => None,
    })
}

#[cfg(test)]
mod tests {
    use crate::staged_list::{StagedList, deserialize_expanded_list};
    use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, attribute_name::AttributeName};
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Debug, Deserialize)]
    struct Carrier {
        #[serde(default, deserialize_with = "deserialize_expanded_list::<AttributeName, _, _>")]
        names: Option<Vec<AttributeIri>>,
    }

    #[test]
    fn a_list_of_absolute_iris_is_read_whole() {
        let carrier: Carrier = serde_json::from_value(json!({"names": ["https://example.com/a", "https://example.com/b"]})).expect("readable");
        pretty_assertions::assert_eq!(carrier.names.map(|names| names.len()), Some(2));
    }

    #[test]
    fn a_list_holding_a_name_that_is_no_absolute_iri_is_held_as_absent_and_no_element_is_kept() {
        let carrier: Carrier = serde_json::from_value(json!({"names": ["https://example.com/a", "bare"]})).expect("readable as written");
        assert!(carrier.names.is_none(), "{carrier:?}");
    }

    #[test]
    fn an_element_that_is_no_name_in_either_stage_fails_the_list() {
        assert!(serde_json::from_value::<Carrier>(json!({"names": [5]})).is_err());
        assert!(serde_json::from_value::<Carrier>(json!({"names": ["https://example.com/a", 5]})).is_err());
    }

    #[test]
    fn an_absent_list_stays_absent_and_a_non_list_is_refused() {
        assert!(serde_json::from_value::<Carrier>(json!({})).expect("absent").names.is_none());
        assert!(serde_json::from_value::<Carrier>(json!({"names": "https://example.com/a"})).is_err());
    }

    #[test]
    fn the_staged_list_tells_a_written_list_from_an_expanded_one() {
        let expanded: StagedList<AttributeIri, AttributeName> = serde_json::from_value(json!(["https://example.com/a"])).expect("readable");
        assert!(matches!(expanded, StagedList::Expanded(names) if names.len() == 1));

        let written: StagedList<AttributeIri, AttributeName> = serde_json::from_value(json!(["bare"])).expect("readable");
        assert!(matches!(written, StagedList::Written(names) if names.len() == 1));
    }
}
