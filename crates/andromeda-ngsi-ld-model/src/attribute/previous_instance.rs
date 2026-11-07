//! The instance an attribute was before the change a notification reports.
//!
//! Under `showChanges` (Table 5.2.14-1) a notified instance carries, beside its
//! current value, its family's previous member: `previousValue`,
//! `previousObject`, `previousVocab`, `previousLanguageMap`, `previousJson`,
//! `previousValueList` or `previousObjectList`, each represented as the
//! family's value member is (clauses 4.5.2.2, 4.5.3.2, 4.5.18.2, 4.5.20.2,
//! 4.5.21.2, 4.5.22.2 and 4.5.24.2). Taken out of the instance, that member is
//! the value of an instance of the same family and `datasetId`: the instance as
//! it stood before the change.

use crate::attribute::instance::Attribute;
use std::mem;

impl Attribute {
    /// Take the family's previous member out of this instance, as the
    /// instance it was before the change: this instance's members with the
    /// previous member as the value, without sub-attributes. `None` when the
    /// instance carries no previous member.
    #[must_use]
    pub fn take_previous(&mut self) -> Option<Attribute> {
        if !self.carries_previous() {
            return None;
        }
        let sub_attributes = mem::take(&mut self.common_mut().sub_attributes);
        let mut previous = self.clone(); // clone: the instance before the change keeps every member beside the value
        self.common_mut().sub_attributes = sub_attributes;
        self.forget_previous();
        previous.promote_previous();
        Some(previous)
    }

    /// Whether the instance carries its family's previous member.
    const fn carries_previous(&self) -> bool {
        match self {
            Attribute::Property(property) => property.previous_value.is_some(),
            Attribute::Relationship(relationship) => relationship.previous_object.is_some(),
            Attribute::VocabProperty(vocab_property) => vocab_property.previous_vocab.is_some(),
            Attribute::GeoProperty(geo_property) => geo_property.previous_value.is_some(),
            Attribute::LanguageProperty(language_property) => language_property.previous_language_map.is_some(),
            Attribute::JsonProperty(json_property) => json_property.previous_json.is_some(),
            Attribute::ListProperty(list_property) => list_property.previous_value_list.is_some(),
            Attribute::ListRelationship(list_relationship) => list_relationship.previous_object_list.is_some(),
        }
    }

    /// Drop the family's previous member.
    fn forget_previous(&mut self) {
        match self {
            Attribute::Property(property) => property.previous_value = None,
            Attribute::Relationship(relationship) => relationship.previous_object = None,
            Attribute::VocabProperty(vocab_property) => vocab_property.previous_vocab = None,
            Attribute::GeoProperty(geo_property) => geo_property.previous_value = None,
            Attribute::LanguageProperty(language_property) => language_property.previous_language_map = None,
            Attribute::JsonProperty(json_property) => json_property.previous_json = None,
            Attribute::ListProperty(list_property) => list_property.previous_value_list = None,
            Attribute::ListRelationship(list_relationship) => list_relationship.previous_object_list = None,
        }
    }

    /// Put the family's previous member in place of its value member.
    fn promote_previous(&mut self) {
        match self {
            Attribute::Property(property) => {
                if let Some(previous) = property.previous_value.take() {
                    property.value = previous;
                }
            }
            Attribute::Relationship(relationship) => {
                if let Some(previous) = relationship.previous_object.take() {
                    relationship.object = previous;
                }
            }
            Attribute::VocabProperty(vocab_property) => {
                if let Some(previous) = vocab_property.previous_vocab.take() {
                    vocab_property.vocab = previous;
                }
            }
            Attribute::GeoProperty(geo_property) => {
                if let Some(previous) = geo_property.previous_value.take() {
                    geo_property.value = previous;
                }
            }
            Attribute::LanguageProperty(language_property) => {
                if let Some(previous) = language_property.previous_language_map.take() {
                    language_property.language_map = previous;
                }
            }
            Attribute::JsonProperty(json_property) => {
                if let Some(previous) = json_property.previous_json.take() {
                    json_property.json = previous;
                }
            }
            Attribute::ListProperty(list_property) => {
                if let Some(previous) = list_property.previous_value_list.take() {
                    list_property.value_list = previous;
                }
            }
            Attribute::ListRelationship(list_relationship) => {
                if let Some(previous) = list_relationship.previous_object_list.take() {
                    list_relationship.object_list = previous;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute::instance::Attribute;
    use serde_json::{Value, json};

    /// `instance` read as the compact model reads an attribute node.
    fn attribute(instance: &Value) -> Attribute {
        serde_json::from_value(instance.clone()).expect("an attribute instance")
    }

    /// `instance` written back.
    fn written(instance: &Attribute) -> Value {
        serde_json::to_value(instance).expect("the instance writes")
    }

    /// Table 5.2.14-1 `showChanges`: the previous member is the value of the
    /// instance before the change, which keeps its `datasetId` and drops the
    /// sub-attributes, and the notified instance keeps its current value alone.
    #[test]
    fn a_previous_value_becomes_the_value_of_the_instance_before_the_change() {
        let mut notified = attribute(&json!({
            "type": "Property",
            "value": 5,
            "previousValue": 4,
            "datasetId": "urn:ngsi-ld:Dataset:a",
            "accuracy": {"type": "Property", "value": 0.1}
        }));
        let previous = notified.take_previous().expect("a previous instance");
        pretty_assertions::assert_eq!(
            written(&previous),
            json!({"type": "Property", "value": 4, "datasetId": "urn:ngsi-ld:Dataset:a"})
        );
        pretty_assertions::assert_eq!(
            written(&notified),
            json!({"type": "Property", "value": 5, "datasetId": "urn:ngsi-ld:Dataset:a", "accuracy": {"type": "Property", "value": 0.1}})
        );
    }

    /// Clause 4.5.3.2: a Relationship's previous member is `previousObject`.
    #[test]
    fn a_previous_object_becomes_the_object_of_the_relationship_before_the_change() {
        let mut notified = attribute(&json!({"type": "Relationship", "object": "urn:ngsi-ld:Person:b", "previousObject": "urn:ngsi-ld:Person:a"}));
        let previous = notified.take_previous().expect("a previous instance");
        pretty_assertions::assert_eq!(written(&previous), json!({"type": "Relationship", "object": "urn:ngsi-ld:Person:a"}));
    }

    #[test]
    fn an_instance_without_a_previous_member_gives_none() {
        let mut notified = attribute(&json!({"type": "Property", "value": 5}));
        assert!(notified.take_previous().is_none());
    }
}
