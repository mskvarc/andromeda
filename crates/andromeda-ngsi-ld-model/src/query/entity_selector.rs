//! Entity selection within a clause-5.2.23 query body.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::{pattern::Pattern, type_selection::grammar::TypeExprRaw};
use serde::{Deserialize, Deserializer, Serialize};

/// One entity selector (ETSI GS CIM 009 v1.9.1 clause 5.2.33).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryEntitySelector {
    /// Entity identifiers admitted by this selector.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "deserialize_optional_ids")]
    pub id: Option<Vec<Uri>>,
    /// Regular expression applied to entity identifiers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_pattern: Option<Pattern>,
    /// Required clause-4.17 type-selection expression.
    #[serde(rename = "type")]
    pub type_: TypeExprRaw,
}

/// Accept the String-or-String-array union defined by Table 5.2.33-1.
fn deserialize_optional_ids<'de, D>(deserializer: D) -> Result<Option<Vec<Uri>>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        /// One URI.
        One(Uri),
        /// Several URIs.
        Many(Vec<Uri>),
    }

    Option::<OneOrMany>::deserialize(deserializer).map(|value| {
        value.map(|ids| match ids {
            OneOrMany::One(id) => vec![id],
            OneOrMany::Many(ids) => ids,
        })
    })
}
