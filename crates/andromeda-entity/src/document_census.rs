//! Counting what a written document costs against the payload budget.
//!
//! ETSI GS CIM 009 v1.9.1 sets no size for an entity or a geometry; clause
//! 6.3.2 answers a request whose input "was too large" with 413
//! `RequestEntityTooLarge`. The budget bounds what one write makes the broker
//! hold, and it is counted on the written document the expansion produced: the
//! typed expansion does not multiply a payload as a JSON-LD node tree would, so
//! the count is the same before and after it. Two things are counted:
//!
//! - the positions of every `GeoProperty` geometry (clause 4.7), in its object
//!   form or encoded as a JSON string (clause 4.7.2), each geometry against
//!   the position limit;
//! - every other value the document holds, against the entity's value limit.
//!   A JSON literal (clause 4.5.24) counts once whatever it holds, as it is
//!   kept verbatim.

use andromeda_ngsi_ld_entity::{
    document::{
        EntityDocument,
        attribute_member::AttributeMember,
        instance_document::InstanceDocument,
        slot::Slot,
        written_value::{ValueSlot, WrittenValue},
    },
    expanded_value::ExpandedValue,
    geo_value::GeoValue,
    value_mode::{Fragment, FragmentMember},
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_number::json_value::value_census::ValueTally;
use andromeda_ngsi_ld_representation::payload_budget::{EntityPayloadBudget, GeometryOwner, PayloadBudgetExceeded};
use indexmap::IndexMap;

/// Admit a written entity document whose census stays within `budget`.
///
/// # Errors
///
/// Returns the first [`PayloadBudgetExceeded`] the census meets: a geometry
/// over the position limit, or the value count passing the entity limit.
pub fn admit_entity_document(budget: EntityPayloadBudget, document: &EntityDocument) -> Result<(), PayloadBudgetExceeded> {
    let census = DocumentCensus::new(budget);
    census.count(1 + usize::from(document.id().is_some()) + document.types().len())?;
    if let Some(scope) = document.scope() {
        census.count(match scope {
            Slot::Set(scopes) => scopes.as_slice().len(),
            Slot::Remove => 1,
        })?;
    }
    census.count(usize::from(document.expires_at().is_some()))?;
    census.members(document.attributes())
}

/// Admit the written instance clauses 5.6.4 and 5.6.19 write to `attribute`,
/// whose census stays within `budget`.
///
/// # Errors
///
/// Returns the first [`PayloadBudgetExceeded`] the census meets, on the same
/// terms as [`admit_entity_document`].
pub fn admit_instance_document(budget: EntityPayloadBudget, attribute: &AttributeName, instance: &InstanceDocument) -> Result<(), PayloadBudgetExceeded> {
    DocumentCensus::new(budget).instance(attribute, instance)
}

/// The running count of one document's census.
struct DocumentCensus {
    /// The limits the document is counted against.
    budget: EntityPayloadBudget,
    /// The values counted so far outside geometry positions.
    values: ValueTally,
}

impl DocumentCensus {
    /// A census of nothing yet, under `budget`.
    const fn new(budget: EntityPayloadBudget) -> DocumentCensus {
        DocumentCensus {
            budget,
            values: ValueTally::new(budget.json_values),
        }
    }

    /// Count `values` values, refusing once the count passes the entity limit.
    fn count(&self, values: usize) -> Result<(), PayloadBudgetExceeded> {
        (0..values).try_for_each(|_| {
            self.values.admit_one().map_err(|_exceeded| PayloadBudgetExceeded::EntityJsonValues {
                limit: self.budget.json_values,
            })
        })
    }

    /// Count every attribute member of an entity or an instance.
    fn members(&self, members: &IndexMap<AttributeName, AttributeMember>) -> Result<(), PayloadBudgetExceeded> {
        members.iter().try_for_each(|(name, member)| match member {
            AttributeMember::Remove => self.count(1),
            AttributeMember::Instances(instances) => {
                self.count(1)?;
                instances.iter().try_for_each(|instance| self.instance(name, instance))
            }
        })
    }

    /// Count one written instance of `attribute`.
    fn instance(&self, attribute: &AttributeName, instance: &InstanceDocument) -> Result<(), PayloadBudgetExceeded> {
        let members = [
            instance.family().is_some(),
            instance.dataset_id().is_some(),
            instance.observed_at().is_some(),
            instance.expires_at().is_some(),
            instance.unit_code().is_some(),
            instance.value_type().is_some(),
            instance.deleted_at().is_some(),
            instance.instance_id().is_some(),
        ];
        let object_types = instance.object_types().map_or(0, |slot| match slot {
            Slot::Set(types) => types.len(),
            Slot::Remove => 1,
        });
        self.count(1 + members.into_iter().filter(|held| *held).count() + object_types)?;
        match instance.value() {
            ValueSlot::Absent => Ok(()),
            ValueSlot::Remove(_) => self.count(1),
            ValueSlot::Set(value) => self.written_value(attribute, value),
        }?;
        self.members(instance.sub_attributes())
    }

    /// Count the value one instance writes.
    fn written_value(&self, attribute: &AttributeName, value: &WrittenValue) -> Result<(), PayloadBudgetExceeded> {
        match value {
            WrittenValue::Value(values) => values.iter().try_for_each(|value| self.expanded_value(value)),
            WrittenValue::ValueList(items) => {
                self.count(1)?;
                items.iter().try_for_each(|item| self.expanded_value(item))
            }
            WrittenValue::Geometry(geometry) => self.geometry(attribute, geometry),
            WrittenValue::Object(objects) => self.count(objects.len()),
            WrittenValue::LanguageMap(entries) => self.count(1 + entries.entries().len()),
            WrittenValue::Vocab(vocab) => self.count(vocab.len()),
            WrittenValue::Json(literals) => self.count(literals.len()),
            WrittenValue::ObjectList(objects) => self.count(1 + objects.len()),
        }
    }

    /// Count one geometry: its positions against the position limit, the
    /// geometry itself as one value.
    fn geometry(&self, attribute: &AttributeName, geometry: &GeoValue) -> Result<(), PayloadBudgetExceeded> {
        if !self.budget.geometry_positions.admits(geometry.form().position_count()) {
            return Err(PayloadBudgetExceeded::GeometryPositions {
                owner: GeometryOwner::Attribute(attribute.clone()), // clone: the refusal names the attribute it refuses
                limit: self.budget.geometry_positions,
            });
        }
        self.count(1)
    }

    /// Count one value of a Property or a list, and every value below it.
    fn expanded_value(&self, value: &ExpandedValue<Fragment>) -> Result<(), PayloadBudgetExceeded> {
        self.count(1)?;
        match value {
            ExpandedValue::Literal(_) | ExpandedValue::Json(_) => Ok(()),
            ExpandedValue::List(items) => items.iter().try_for_each(|item| self.expanded_value(item)),
            ExpandedValue::Node(node) => node.members().values().try_for_each(|member| match member {
                FragmentMember::Values(values) => values.iter().try_for_each(|value| self.expanded_value(value)),
                FragmentMember::Remove => self.count(1),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::document_census::{admit_entity_document, admit_instance_document};
    use andromeda_ngsi_ld_entity::document::{EntityDocument, instance_document::InstanceDocument};
    use andromeda_ngsi_ld_geometry::geometry_census::GeometryPositionLimit;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use andromeda_ngsi_ld_representation::{
        attribute_nesting::AttributeNestingLimit,
        payload_budget::{EntityPayloadBudget, GeometryOwner, PayloadBudgetExceeded},
    };
    use serde_json::{Value, json};
    use std::num::NonZeroUsize;

    fn budget(values: usize, positions: usize) -> EntityPayloadBudget {
        EntityPayloadBudget {
            json_values: JsonValueLimit::new(NonZeroUsize::new(values).expect("a positive value limit")),
            geometry_positions: GeometryPositionLimit::new(NonZeroUsize::new(positions).expect("a positive position limit")),
            attribute_nesting: AttributeNestingLimit::DECODABLE_CEILING,
        }
    }

    fn document(expanded: &Value) -> EntityDocument {
        EntityDocument::from_slice(&serde_json::to_vec(expanded).expect("JSON")).expect("a written document")
    }

    /// A Vehicle located by a square: one ring of five positions.
    fn located() -> EntityDocument {
        document(&json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://example.org/Vehicle"],
            "https://uri.etsi.org/ngsi-ld/location": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "@type": ["https://purl.org/geojson/vocab#Polygon"],
                    "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@list": [
                        {"@list": [{"@value": 0}, {"@value": 0}]},
                        {"@list": [{"@value": 1}, {"@value": 0}]},
                        {"@list": [{"@value": 1}, {"@value": 1}]},
                        {"@list": [{"@value": 0}, {"@value": 1}]},
                        {"@list": [{"@value": 0}, {"@value": 0}]}
                    ]}]}]
                }]
            }]
        }]))
    }

    /// Clause 6.3.2: a geometry over the position limit is refused, naming
    /// its attribute; one at the limit is admitted.
    #[test]
    fn a_geometry_is_counted_against_the_position_limit() {
        let refusal = admit_entity_document(budget(1_000, 4), &located()).expect_err("five positions exceed four");
        let PayloadBudgetExceeded::GeometryPositions {
            owner: GeometryOwner::Attribute(owner),
            ..
        } = refusal
        else {
            panic!("the refusal names the geometry: {refusal:?}");
        };
        pretty_assertions::assert_eq!(owner.as_str(), "https://uri.etsi.org/ngsi-ld/location");
        admit_entity_document(budget(1_000, 5), &located()).expect("five positions are within five");
    }

    /// Every item of a Property value counts against the entity limit, so a
    /// value list longer than the limit is refused.
    #[test]
    fn every_value_of_a_property_counts_against_the_entity_limit() {
        let items: Vec<Value> = (0..20).map(|item| json!({"@value": item})).collect();
        let numbers = document(&json!([{
            "@id": "urn:ngsi-ld:Probe:1",
            "@type": ["https://example.org/Probe"],
            "https://example.org/n": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": items}]
        }]));
        assert!(matches!(
            admit_entity_document(budget(10, 10), &numbers),
            Err(PayloadBudgetExceeded::EntityJsonValues { .. })
        ));
        admit_entity_document(budget(100, 10), &numbers).expect("the values fit the limit");
    }

    #[test]
    fn an_attribute_fragment_is_counted_like_an_entity_attribute() {
        let attribute: AttributeName = "https://example.org/n".parse().expect("a valid attribute name");
        let fragment = InstanceDocument::from_slice(
            &attribute,
            &serde_json::to_vec(&json!({
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}, {"@value": 2}, {"@value": 3}]
            }))
            .expect("JSON"),
        )
        .expect("a written instance");
        assert!(admit_instance_document(budget(3, 10), &attribute, &fragment).is_err());
        admit_instance_document(budget(10, 10), &attribute, &fragment).expect("the values fit the limit");
    }
}
