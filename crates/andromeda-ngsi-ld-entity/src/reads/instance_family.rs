//! The family of each attribute instance, keyed by its identity (clause
//! 4.5.5): what the search projection and the change records read.

use crate::{entity_state::EntityState, instance::Instance, reads::attribute_instance_key::AttributeInstanceKey};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier};

impl EntityState {
    /// The family of every attribute instance, with its identity.
    pub fn instance_families(&self) -> impl Iterator<Item = (&AttributeName, Option<&Identifier>, AttributeType)> {
        self.attributes()
            .iter()
            .flat_map(|(name, instances)| instances.iter().map(move |(dataset_id, instance)| (name, dataset_id, instance.family())))
    }

    /// The identity of every attribute instance.
    pub fn instance_keys(&self) -> impl Iterator<Item = AttributeInstanceKey> + '_ {
        self.instance_families().map(|(name, dataset_id, _)| AttributeInstanceKey {
            attr: name.clone(),
            dataset_id: dataset_id.cloned(),
        })
    }

    /// The family of each instance of the attribute `name`, in stored order;
    /// nothing when the entity has no such attribute.
    pub fn member_instance_families(&self, name: &str) -> impl Iterator<Item = AttributeType> + '_ {
        self.attributes()
            .get(name)
            .into_iter()
            .flat_map(|instances| instances.iter().map(|(_, instance)| instance.family()))
    }

    /// The family of the instance of `name` with `dataset_id`, when it exists.
    #[must_use]
    pub fn attribute_instance_family(&self, name: &str, dataset_id: Option<&Identifier>) -> Option<AttributeType> {
        self.attributes().get(name)?.get(dataset_id).map(Instance::family)
    }
}

#[cfg(test)]
mod tests {
    use crate::entity_state::EntityState;
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
    use serde_json::json;

    fn state() -> EntityState {
        let document = json!([{
            "@id": "urn:ngsi-ld:E:1",
            "@type": ["https://ex.org/T"],
            "https://ex.org/b": [
                {"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]},
                {"@type": ["https://uri.etsi.org/ngsi-ld/Relationship"], "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:x:y"}], "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:d"}]}
            ]
        }]);
        EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a state")
    }

    #[test]
    fn each_declared_family_is_carried_beside_the_instance_identity() {
        let state = state();
        pretty_assertions::assert_eq!(
            state.member_instance_families("https://ex.org/b").collect::<Vec<_>>(),
            vec![AttributeType::Property, AttributeType::Relationship]
        );
        let dataset = "urn:ngsi-ld:Dataset:d".parse().expect("an identifier");
        pretty_assertions::assert_eq!(
            state.attribute_instance_family("https://ex.org/b", Some(&dataset)),
            Some(AttributeType::Relationship)
        );
        pretty_assertions::assert_eq!(state.attribute_instance_family("https://ex.org/missing", None), None);
        pretty_assertions::assert_eq!(state.instance_families().count(), 2);
    }
}
