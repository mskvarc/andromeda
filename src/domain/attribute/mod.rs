mod geo_property;
mod json_property;
mod language_property;
mod list_property;
mod list_relationship;
mod property;
mod relationship;
mod vocab_property;

pub use geo_property::GeoProperty;
pub use json_property::JsonProperty;
pub use language_property::LanguageProperty;
pub use list_property::ListProperty;
pub use list_relationship::ListRelationship;
pub use property::Property;
pub use relationship::Relationship;
pub use vocab_property::VocabProperty;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Attribute {
    Property(Property),
    Relationship(Relationship),
    VocabProperty(VocabProperty),
    GeoProperty(GeoProperty),
    LanguageProperty(LanguageProperty),
    JsonProperty(JsonProperty),
    ListProperty(ListProperty),
    ListRelationship(ListRelationship),
}

impl Attribute {
    pub fn value(&self) -> serde_json::Value {
        match self {
            Attribute::Property(p) => p.value.clone(),
            Attribute::Relationship(r) => serde_json::Value::String(r.object.to_string()),
            Attribute::VocabProperty(v) => v.vocab.clone(),
            Attribute::GeoProperty(g) => serde_json::json!(g.value),
            Attribute::LanguageProperty(l) => l.language_map.clone(),
            Attribute::JsonProperty(j) => j.json.clone(),
            Attribute::ListProperty(lp) => serde_json::json!(lp.value_list),
            Attribute::ListRelationship(lr) => serde_json::json!(lr.object_list.iter().map(|u| u.to_string()).collect::<Vec<_>>()),
        }
    }

    pub fn dataset_id_string(&self) -> Option<String> {
        let uri = match self {
            Attribute::Property(p) => p.dataset_id.as_ref(),
            Attribute::Relationship(r) => r.dataset_id.as_ref(),
            Attribute::VocabProperty(v) => v.dataset_id.as_ref(),
            Attribute::GeoProperty(g) => g.dataset_id.as_ref(),
            Attribute::LanguageProperty(l) => l.dataset_id.as_ref(),
            Attribute::JsonProperty(j) => j.dataset_id.as_ref(),
            Attribute::ListProperty(lp) => lp.dataset_id.as_ref(),
            Attribute::ListRelationship(lr) => lr.dataset_id.as_ref(),
        };
        uri.map(|u| u.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AttributeValue {
    Single(Attribute),
    Multiple(Vec<Attribute>),
    Raw(serde_json::Value),
}
