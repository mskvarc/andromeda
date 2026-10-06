//! Entity selection within a clause-5.2.23 query body.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{pattern::Pattern, type_selection::grammar::TypeExprRaw};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// One entity selector (ETSI GS CIM 009 v1.9.1 clause 5.2.33).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryEntitySelector {
    /// Entity identifiers admitted by this selector.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_ids",
        serialize_with = "serialize_optional_ids"
    )]
    pub id: Option<Vec<Identifier>>,
    /// Regular expression applied to entity identifiers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_pattern: Option<Pattern>,
    /// Required clause-4.17 type-selection expression.
    #[serde(rename = "type")]
    pub type_: TypeExprRaw,
}

/// Write the String-or-String-array union of Table 5.2.33-1 the way the
/// shortest form states it: one identifier as a String, several as an array.
///
/// The function is public because it names the wire form of `id`: it is the
/// encoding the member has, and serde's `serialize_with` hands it the member
/// as the `Option` the field holds.
///
/// # Errors
///
/// Returns the serializer's own error when the member cannot be written.
pub fn serialize_optional_ids<S>(ids: &Option<Vec<Identifier>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match ids.as_deref() {
        Some([only]) => only.serialize(serializer),
        Some(ids) => ids.serialize(serializer),
        None => serializer.serialize_none(),
    }
}

/// Accept the String-or-String-array union defined by Table 5.2.33-1.
fn deserialize_optional_ids<'de, D>(deserializer: D) -> Result<Option<Vec<Identifier>>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        /// One URI.
        One(Identifier),
        /// Several URIs.
        Many(Vec<Identifier>),
    }

    Option::<OneOrMany>::deserialize(deserializer).map(|value| {
        value.map(|ids| match ids {
            OneOrMany::One(id) => vec![id],
            OneOrMany::Many(ids) => ids,
        })
    })
}
