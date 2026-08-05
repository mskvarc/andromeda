//! The `objectList` / `previousObjectList` member (ETSI GS CIM 009 clause 5.2.37).

use andromeda_ngsi_ld_lexicon::uri::{Uri, UriParseError};
use derive_more::{AsRef, Deref, From};
use serde::{Deserialize, Deserializer, Serialize, de::Error as DeError};
use serde_json::Value;
use std::{slice::Iter, str::FromStr, vec::IntoIter};
use thiserror::Error;

/// `objectList` / `previousObjectList` value as defined by clause 5.2.37.
///
/// `#[serde(transparent)]` gives the serialise half — a bare JSON array of URI
/// strings. The deserialise half stays hand-written below because it validates
/// each entry, which no derive expresses.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, AsRef, Deref, From)]
#[serde(transparent)]
pub struct RelationshipObjectList(pub Vec<Uri>);

impl<'de> Deserialize<'de> for RelationshipObjectList {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<RelationshipObjectList, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let items = value
            .as_array()
            .ok_or_else(|| DeError::custom("objectList must be an array"))?
            .iter()
            .map(parse_relationship_object_entry)
            .collect::<Result<Vec<_>, _>>()
            .map_err(DeError::custom)?;
        Ok(RelationshipObjectList(items))
    }
}

impl<'a> IntoIterator for &'a RelationshipObjectList {
    type IntoIter = Iter<'a, Uri>;
    type Item = &'a Uri;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl IntoIterator for RelationshipObjectList {
    type IntoIter = IntoIter<Uri>;
    type Item = Uri;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// Failures parsing one entry of an NGSI-LD `objectList` (clause 5.2.37).
#[derive(Debug, Error)]
pub enum RelationshipObjectEntryError {
    /// The entry was neither a URI string nor an `{"object": …}` wrapper.
    #[error("relationship object list entries must be strings or {{\"object\": ...}} objects")]
    NotAnObjectReference,
    /// The entry held a value that is not a valid URI.
    #[error("invalid relationship object URI: {source}")]
    Uri {
        /// Underlying URI parse failure.
        #[source]
        source: UriParseError,
    },
}

/// Read one `objectList` entry, accepting both the bare URI string and the
/// `{"object": "…"}` wrapper clause 5.2.37 permits.
///
/// # Errors
///
/// Returns [`RelationshipObjectEntryError::NotAnObjectReference`] when neither
/// shape matches, or [`RelationshipObjectEntryError::Uri`] when the extracted
/// text is not a URI.
pub fn parse_relationship_object_entry(value: &Value) -> Result<Uri, RelationshipObjectEntryError> {
    let raw = match value.as_str() {
        Some(uri) => uri,
        None => value
            .as_object()
            .and_then(|object| object.get("object"))
            .and_then(Value::as_str)
            .ok_or(RelationshipObjectEntryError::NotAnObjectReference)?,
    };
    Uri::from_str(raw).map_err(|source| RelationshipObjectEntryError::Uri { source })
}

#[cfg(test)]
mod tests {
    use crate::attribute::object_list::RelationshipObjectList;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use std::str::FromStr;

    #[test]
    fn relationship_object_list_deserializes_string_entries() {
        let value: RelationshipObjectList = serde_json::from_value(serde_json::json!(["urn:ngsi-ld:Entity:1"])).expect("objectList");
        pretty_assertions::assert_eq!(value.0.len(), 1);
    }

    #[test]
    fn relationship_object_list_deserializes_object_entries() {
        let value: RelationshipObjectList = serde_json::from_value(serde_json::json!([{ "object": "urn:ngsi-ld:Entity:1" }])).expect("objectList");
        pretty_assertions::assert_eq!(value.0.len(), 1);
    }

    #[test]
    fn relationship_object_list_serialises_transparently() {
        // Clause 5.2.37: `objectList` is a plain JSON array of URI strings.
        let list = RelationshipObjectList(vec![
            Uri::from_str("urn:ngsi-ld:Entity:1").expect("uri a"),
            Uri::from_str("urn:ngsi-ld:Entity:2").expect("uri b"),
        ]);
        pretty_assertions::assert_eq!(
            serde_json::to_value(&list).expect("serialises"),
            serde_json::json!(["urn:ngsi-ld:Entity:1", "urn:ngsi-ld:Entity:2"])
        );
    }
}
