//! The Temporal Evolution a client writes (clauses 5.6.11 and 5.6.12), from
//! its compact form to the written documents the write records.
//!
//! Two shapes a Temporal Evolution may take have no place in an entity
//! document and are therefore read before the typed expansion runs. A plural
//! member (`values`, `objects`, `languageMaps`, ...) carries several
//! observations under one envelope, each inheriting the envelope's members
//! unless it restates them; it is unfolded into the instances it stands for.
//! A reified Scope (clause 4.5.6: "a temporal representation of a Property")
//! dates the Scope paths it names; it is expanded as the attribute instances
//! it is and recorded beside the entity document, keyed by the scope IRI as the
//! temporal representation keys it.

use crate::{error::TemporalError, temporal_validation_reason::TemporalValidationReason, unit_code_admission::admit_document_unit_codes};
use andromeda_context_commit::history_record::HistoryRecord;
use andromeda_entity::document_census::admit_entity_document;
use andromeda_ngsi_ld_entity::document::{EntityDocument, EntityDocumentParts, attribute_member::AttributeMember};
use andromeda_ngsi_ld_expansion::{entity_expansion::EntityExpansion, normalization::Normalization};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_representation::payload_budget::{EntityPayloadBudget, PayloadBudgetExceeded};
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, expanded::properties as expanded_properties};
use indexmap::IndexMap;
use serde_json::{Map, Value};
use std::sync::Arc;

/// A written Temporal Evolution, expanded.
#[derive(Debug)]
pub(crate) struct WrittenEvolution {
    /// The instances the evolution adds, with its identifier, types and any
    /// non-reified Scope.
    document: Arc<EntityDocument>,
    /// The reified Scope instances the evolution dates, keyed by the scope
    /// IRI; absent when it dates none.
    reified_scope: Option<Arc<EntityDocument>>,
}

impl WrittenEvolution {
    /// The evolution adding the instances of `document`, and dating the reified
    /// Scope instances of `reified_scope`, keyed by the scope IRI.
    pub(crate) const fn new(document: Arc<EntityDocument>, reified_scope: Option<Arc<EntityDocument>>) -> WrittenEvolution {
        WrittenEvolution { document, reified_scope }
    }

    /// Expand the compact Temporal Evolution `payload` under `expansion`.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::JsonLd`] when the context does not resolve or
    /// the processor refuses the payload, and [`TemporalError::Validation`]
    /// when the payload is no Temporal Evolution the model admits or puts a
    /// `unitCode` on a family defined without one.
    pub(crate) async fn expand(expansion: &EntityExpansion<'_>, mut payload: Value) -> Result<WrittenEvolution, TemporalError> {
        unfold_plural_members(&mut payload);
        let reified = take_reified_scope(&mut payload);
        let document = expansion.entity_fragment(payload, Normalization::Entity).await?;
        admit_document_unit_codes(&document)?;
        let reified_scope = match reified {
            Some(instances) => Some(Arc::new(expand_reified_scope(expansion, instances).await?)),
            None => None,
        };
        Ok(WrittenEvolution::new(Arc::new(document), reified_scope))
    }

    /// The instances the evolution adds.
    pub(crate) const fn document(&self) -> &Arc<EntityDocument> {
        &self.document
    }

    /// Hold the written evolution to the budget an entity is admitted under
    /// (clause 6.3.2, "Request Entity too large"), the Scope it dates included.
    ///
    /// # Errors
    ///
    /// Returns [`PayloadBudgetExceeded`] when either document exceeds it.
    pub(crate) fn admit_budget(&self, budget: EntityPayloadBudget) -> Result<(), PayloadBudgetExceeded> {
        admit_entity_document(budget, &self.document)?;
        self.reified_scope.iter().try_for_each(|scope| admit_entity_document(budget, scope))
    }

    /// The evolution addressed to `entity_id`, of the `recorded_types` its
    /// Temporal Evolution records, where the written fragment names neither
    /// (clause 5.6.12 addresses the evolution by the request path).
    pub(crate) fn addressed_to(self, entity_id: &Identifier, recorded_types: Vec<EntityTypeName>) -> WrittenEvolution {
        let WrittenEvolution { document, reified_scope } = self;
        let EntityDocumentParts {
            id,
            types,
            scope,
            expires_at,
            attributes,
        } = Arc::unwrap_or_clone(document).into_parts();
        let id = id.or_else(|| Some(entity_id.clone())); // clone: the document owns the identifier the request path names
        let types = if types.is_empty() { recorded_types } else { types };
        WrittenEvolution::new(Arc::new(EntityDocument::new(id, types, scope, expires_at, attributes)), reified_scope)
    }

    /// The history the write records: every instance the evolution adds, and
    /// the Scope instances it dates.
    pub(crate) fn history(&self) -> Vec<HistoryRecord> {
        let mut history = vec![HistoryRecord::EvolutionInstances(Arc::clone(&self.document))]; // clone: Arc ref-count bump
        history.extend(self.reified_scope.iter().map(|scope| HistoryRecord::EvolutionInstances(Arc::clone(scope)))); // clone: Arc ref-count bump
        history
    }
}

/// Expand each reified Scope instance as the attribute instance it is, into a
/// document holding them under the scope IRI alone.
async fn expand_reified_scope(expansion: &EntityExpansion<'_>, instances: Vec<Value>) -> Result<EntityDocument, TemporalError> {
    let written_name = compact_properties::SCOPE
        .parse::<AttributeName>()
        .map_err(|source| TemporalError::from(TemporalValidationReason::AttributeName { source }))?;
    let mut expanded = Vec::with_capacity(instances.len());
    for instance in instances {
        expanded.push(expansion.attribute_fragment(&written_name, instance).await?.into_parts().1);
    }
    let scope = expanded_properties::SCOPE
        .as_str()
        .parse::<AttributeName>()
        .map_err(|source| TemporalError::from(TemporalValidationReason::AttributeName { source }))?;
    Ok(EntityDocument::new(
        None,
        Vec::new(),
        None,
        None,
        IndexMap::from([(scope, AttributeMember::Instances(expanded))]),
    ))
}

/// Take the reified Scope out of `payload`: a `scope` member holding one
/// instance object, or an array of them. A Scope written as paths stays where
/// it is, for the entity document to carry.
fn take_reified_scope(payload: &mut Value) -> Option<Vec<Value>> {
    let members = payload.as_object_mut()?;
    let reified = match members.get(compact_properties::SCOPE)? {
        Value::Object(_) => true,
        Value::Array(items) => !items.is_empty() && items.iter().all(Value::is_object),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    };
    if !reified {
        return None;
    }
    match members.remove(compact_properties::SCOPE)? {
        Value::Array(items) => Some(items),
        single @ (Value::Object(_) | Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => Some(vec![single]),
    }
}

/// Unfold every plural member of `payload` into the instances it stands for.
///
/// An instance holding its family's plural member (clause 4.5.9: `values`,
/// `objects`, `languageMaps`, `vocabs`, `jsons`, `valueLists`, `objectLists`)
/// becomes one instance per element: the envelope's members without the
/// plural one, overlaid by the element's own, so a member an element restates
/// overrides the envelope's.
fn unfold_plural_members(payload: &mut Value) {
    let Some(members) = payload.as_object_mut() else {
        return;
    };
    for member in members.values_mut() {
        let unfolds = match member {
            Value::Object(instance) => plural_key(instance).is_some(),
            Value::Array(instances) => instances
                .iter()
                .any(|instance| instance.as_object().is_some_and(|instance| plural_key(instance).is_some())),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
        };
        if !unfolds {
            continue;
        }
        let instances = match member.take() {
            Value::Array(instances) => instances,
            single @ (Value::Object(_) | Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => vec![single],
        };
        *member = Value::Array(instances.into_iter().flat_map(unfold_instance).collect());
    }
}

/// The plural member an instance's declared family holds its observations in,
/// when the instance holds it.
fn plural_key(instance: &Map<String, Value>) -> Option<&'static str> {
    let family = instance.get("type").and_then(Value::as_str)?.parse::<AttributeType>().ok()?;
    let key = family.temporal_values_key();
    instance.contains_key(key).then_some(key)
}

/// The instances one instance stands for: itself, or one per element of its
/// plural member.
fn unfold_instance(instance: Value) -> Vec<Value> {
    let Value::Object(mut envelope) = instance else {
        return vec![instance];
    };
    let Some(key) = plural_key(&envelope) else {
        return vec![Value::Object(envelope)];
    };
    match envelope.remove(key) {
        Some(Value::Array(observations)) => observations
            .into_iter()
            .map(|observation| {
                let mut unfolded = envelope.clone(); // clone: every observation inherits the envelope's members
                if let Value::Object(own) = observation {
                    unfolded.extend(own);
                }
                Value::Object(unfolded)
            })
            .collect(),
        Some(other) => {
            envelope.insert(key.to_owned(), other);
            vec![Value::Object(envelope)]
        }
        None => vec![Value::Object(envelope)],
    }
}

#[cfg(test)]
mod tests {
    use crate::written_evolution::{take_reified_scope, unfold_plural_members};
    use serde_json::json;

    /// Clause 4.5.9: each element of a plural member is its own observation,
    /// inheriting the envelope's members and overriding the ones it restates.
    #[test]
    fn a_plural_member_unfolds_into_one_instance_per_element() {
        let mut payload = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {
                "type": "Property",
                "unitCode": "KMH",
                "datasetId": "urn:ngsi-ld:Dataset:sensor",
                "values": [
                    {"value": 55, "observedAt": "2021-01-01T00:00:00Z"},
                    {"value": 61, "observedAt": "2021-01-02T00:00:00Z", "unitCode": "MTS"}
                ]
            },
            "brand": {"type": "Property", "value": "Mercedes"}
        });
        unfold_plural_members(&mut payload);
        pretty_assertions::assert_eq!(
            payload,
            json!({
                "id": "urn:ngsi-ld:Vehicle:1",
                "type": "Vehicle",
                "speed": [
                    {"type": "Property", "unitCode": "KMH", "datasetId": "urn:ngsi-ld:Dataset:sensor", "value": 55, "observedAt": "2021-01-01T00:00:00Z"},
                    {"type": "Property", "unitCode": "MTS", "datasetId": "urn:ngsi-ld:Dataset:sensor", "value": 61, "observedAt": "2021-01-02T00:00:00Z"}
                ],
                "brand": {"type": "Property", "value": "Mercedes"}
            })
        );
    }

    /// The two list families hold their observations under their own plural
    /// members (clause 4.5.9), which unfold the same way.
    #[test]
    fn a_list_family_unfolds_its_own_plural_member() {
        let mut payload = json!({
            "parts": [{"type": "ListRelationship", "objectLists": [{"objectList": [{"object": "urn:ngsi-ld:Part:1"}]}]}]
        });
        unfold_plural_members(&mut payload);
        pretty_assertions::assert_eq!(
            payload,
            json!({"parts": [{"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:Part:1"}]}]})
        );
    }

    /// Clause 4.5.6: a reified Scope is taken out to be dated; a Scope written
    /// as paths stays in the entity.
    #[test]
    fn only_a_reified_scope_is_taken_out() {
        let mut reified = json!({"id": "urn:ngsi-ld:V:1", "scope": {"type": "Property", "value": "/Madrid", "observedAt": "2020-01-01T00:00:00Z"}});
        pretty_assertions::assert_eq!(
            take_reified_scope(&mut reified),
            Some(vec![json!({"type": "Property", "value": "/Madrid", "observedAt": "2020-01-01T00:00:00Z"})])
        );
        assert!(reified.get("scope").is_none());

        let mut paths = json!({"id": "urn:ngsi-ld:V:1", "scope": ["/Madrid", "/Company"]});
        pretty_assertions::assert_eq!(take_reified_scope(&mut paths), None);
        pretty_assertions::assert_eq!(paths["scope"], json!(["/Madrid", "/Company"]));
    }
}
