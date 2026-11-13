//! The Temporal Evolution a client writes (clauses 5.6.11 and 5.6.12), from
//! its typed compact form to the written documents the write records.
//!
//! The body arrives decoded ([`WrittenTemporalEntity`]): an attribute in the
//! simplified temporal representation (clauses 4.5.9 and 5.2.20), which holds
//! its instances as `[value, instant]` pairs under its family's plural member,
//! was unfolded into the instances the pairs stand for as the body was read.
//! What is left to do here is what two shapes of a Temporal Evolution need that
//! an entity document does not hold: a reified Scope (clause 4.5.6: "a temporal
//! representation of a Property") dates the Scope paths it names, so it is
//! expanded as the attribute instances it is and recorded beside the entity
//! document, keyed by the scope IRI as the temporal representation keys it.

use crate::{
    error::TemporalError,
    reified_scope::scope_observations,
    temporal_validation_reason::TemporalValidationReason,
    unit_code_admission::admit_document_unit_codes,
};
use andromeda_context_commit::history_record::HistoryRecord;
use andromeda_entity::document_census::admit_entity_document;
use andromeda_ngsi_ld_entity::{
    admission::{attribute_nesting_admission::admit_document_nesting, null_sentinel_admission::NullSentinelAdmission},
    document::{EntityDocument, EntityDocumentParts, attribute_member::AttributeMember},
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_expansion::{entity_expansion::EntityExpansion, normalization::Normalization};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    attribute::instance::{Attribute, AttributeValue},
    temporal::{temporal_document::TemporalDocument, written_temporal_entity::WrittenTemporalEntity},
};
use andromeda_ngsi_ld_representation::payload_budget::EntityPayloadBudget;
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, expanded::properties as expanded_properties};
use indexmap::IndexMap;
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

    /// Expand the Temporal Evolution `written` under `expansion`.
    ///
    /// Clause 5.5.4 excepts the temporal evolution from its second NGSI-LD
    /// Null prohibition, so a deleted instance is written as one, but not from
    /// the third: no Property value written here holds the Null under a key.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::JsonLd`] when the context does not resolve or
    /// the processor refuses the payload, and [`TemporalError::Validation`]
    /// when the payload is no Temporal Evolution the model admits, puts a
    /// `unitCode` on a family defined without one, writes a geospatial member
    /// as another family than `GeoProperty`, or holds the NGSI-LD Null under a
    /// key of a Property value.
    pub(crate) async fn expand(expansion: &EntityExpansion<'_>, written: WrittenTemporalEntity) -> Result<WrittenEvolution, TemporalError> {
        let (fragment, reified) = written.into_fragment();
        let document = expansion.entity_patch(&fragment, Normalization::Entity).await?;
        let reified_scope = match reified {
            Some(instances) => Some(expand_reified_scope(expansion, instances).await?),
            None => None,
        };
        admit_document_unit_codes(&document)?;
        document.admit_geo_members()?;
        document.admit_datatypes()?;
        document.admit_null_placements(NullSentinelAdmission::TemporalEvolution)?;
        Ok(WrittenEvolution::new(Arc::new(document), reified_scope.map(Arc::new)))
    }

    /// The evolution recording `history`, a Temporal Evolution already read
    /// into the model: its Scope history, an attribute of the document under
    /// the Scope IRI, is the reified Scope the write dates, and the rest is
    /// the instances it adds.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Validation`] when an instance puts a
    /// `unitCode` on a family defined without one, writes a geospatial member
    /// as another family than `GeoProperty`, or holds the NGSI-LD Null under a
    /// key of a Property value.
    pub(crate) fn of_history(history: TemporalDocument) -> Result<WrittenEvolution, TemporalError> {
        let (entity, _system_times, _emptied) = history.into_parts();
        let EntityDocumentParts {
            id,
            types,
            scope,
            expires_at,
            mut attributes,
        } = entity.into_parts();
        let reified_scope = attributes
            .shift_remove_entry(expanded_properties::SCOPE.as_str())
            .map(|entry| EntityDocument::new(None, Vec::new(), None, None, IndexMap::from([entry])));
        let document = EntityDocument::new(id, types, scope, expires_at, attributes);
        admit_document_unit_codes(&document)?;
        document.admit_geo_members()?;
        document.admit_datatypes()?;
        document.admit_null_placements(NullSentinelAdmission::TemporalEvolution)?;
        Ok(WrittenEvolution::new(Arc::new(document), reified_scope.map(Arc::new)))
    }

    /// The instances the evolution adds.
    pub(crate) const fn document(&self) -> &Arc<EntityDocument> {
        &self.document
    }

    /// Hold the written evolution to the budget an entity is admitted under
    /// (clause 6.3.2, "Request Entity too large"), the Scope it dates included,
    /// and each of its attributes to the nesting the budget admits.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::PayloadBudget`] when either document exceeds
    /// the budget, and [`TemporalError::AttributeNesting`] when an attribute
    /// nests deeper than it admits.
    pub(crate) fn admit_budget(&self, budget: EntityPayloadBudget) -> Result<(), TemporalError> {
        admit_entity_document(budget, &self.document)?;
        self.reified_scope.iter().try_for_each(|scope| admit_entity_document(budget, scope))?;
        admit_document_nesting(budget.attribute_nesting, &self.document)?;
        Ok(())
    }

    /// The evolution addressed to `entity_id`, of the `recorded_types` its
    /// Temporal Evolution records where the written fragment names none.
    ///
    /// Clause 5.6.12.3 names the target by the request path, and clause 5.4
    /// requires a Fragment's `id` to be equal to it, so the path's identifier
    /// is the evolution's whatever the fragment wrote: an `id` naming the
    /// same Entity in another spelling, or one the admission let through,
    /// never becomes the identifier of an Entity stored under the path.
    pub(crate) fn addressed_to(self, entity_id: &Identifier, recorded_types: Vec<EntityTypeName>) -> WrittenEvolution {
        let WrittenEvolution { document, reified_scope } = self;
        let EntityDocumentParts {
            id: _,
            types,
            scope,
            expires_at,
            attributes,
        } = Arc::unwrap_or_clone(document).into_parts();
        let id = Some(entity_id.clone());
        let types = if types.is_empty() { recorded_types } else { types };
        WrittenEvolution::new(Arc::new(EntityDocument::new(id, types, scope, expires_at, attributes)), reified_scope)
    }

    /// The history the write records: every instance the evolution adds, and
    /// the Scope instances it dates.
    pub(crate) fn history(&self) -> Vec<HistoryRecord> {
        let mut history = vec![HistoryRecord::EvolutionInstances(Arc::clone(&self.document))];
        history.extend(self.reified_scope.iter().map(|scope| HistoryRecord::EvolutionInstances(Arc::clone(scope))));
        history
    }
}

/// Expand each reified Scope instance as the attribute instance it is, into a
/// document holding them under the scope IRI alone, refusing an instance that
/// states no Scope or carries a member clause 4.5.6 does not give it.
pub(crate) async fn expand_reified_scope(expansion: &EntityExpansion<'_>, instances: Vec<Attribute>) -> Result<EntityDocument, TemporalError> {
    let written_name = compact_properties::SCOPE
        .parse::<AttributeName>()
        .map_err(|source| TemporalError::from(TemporalValidationReason::AttributeName { source }))?;
    let mut expanded = Vec::with_capacity(instances.len());
    for instance in instances {
        let member = AttributeValue::Single(Box::new(instance));
        let instance = expansion.attribute(&written_name, &member).await?.into_parts().1;
        scope_observations(&instance).map_err(|refusal| TemporalError::from(EntityRefusal::Scope(refusal)))?;
        expanded.push(instance);
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

#[cfg(test)]
mod tests {
    use crate::{
        error::TemporalError,
        reified_scope_fixture::admitted_scope,
        temporal_validation_reason::TemporalValidationReason,
        written_evolution::WrittenEvolution,
    };
    use andromeda_configuration::schema::request_limits::RequestLimitsConfig;
    use andromeda_entity::configured_policy::payload_budget;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_entity::{document::EntityDocument, history::current_state_fold::fold_into_current_state, refusal::EntityRefusal};
    use andromeda_ngsi_ld_expansion::entity_expansion::EntityExpansion;
    use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
    use andromeda_ngsi_ld_model::{request_body::RequestBody, temporal::written_temporal_entity::WrittenTemporalEntity};
    use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
    use indexmap::IndexMap;
    use serde_json::{Value, json};
    use std::sync::Arc;

    fn identifier(raw: &str) -> Identifier {
        raw.parse().expect("valid identifier")
    }

    /// The evolution a Fragment naming `id` writes, holding no instance.
    fn written(id: Option<Identifier>) -> WrittenEvolution {
        WrittenEvolution::new(Arc::new(EntityDocument::new(id, Vec::new(), None, None, IndexMap::new())), None)
    }

    /// Clause 5.6.12.3: the request path names the target Temporal Evolution
    /// when the Fragment names none.
    #[test]
    fn the_path_names_the_evolution_the_fragment_names_none() {
        let path = identifier("urn:ngsi-ld:Vehicle:1");
        let addressed = written(None).addressed_to(&path, Vec::new());
        pretty_assertions::assert_eq!(addressed.document().id(), Some(&path));
    }

    /// Clauses 5.4 and 5.6.12.3: the target is the path's, whatever the
    /// Fragment names, so a written `id` can never become the identifier of
    /// an Entity stored under the path.
    #[test]
    fn the_path_names_the_evolution_whatever_the_fragment_names() {
        let path = identifier("urn:ngsi-ld:Vehicle:1");
        let addressed = written(Some(identifier("urn:ngsi-ld:Vehicle:2"))).addressed_to(&path, Vec::new());
        pretty_assertions::assert_eq!(addressed.document().id(), Some(&path));
    }

    /// The evolution a Fragment of `types` writes, holding no instance.
    fn written_of(types: &[&str]) -> WrittenEvolution {
        let types = types.iter().map(|raw| raw.parse().expect("a valid type")).collect();
        WrittenEvolution::new(Arc::new(EntityDocument::new(None, types, None, None, IndexMap::new())), None)
    }

    /// The types of `evolution`'s document, as written.
    fn types_of(evolution: &WrittenEvolution) -> Vec<&str> {
        evolution.document().types().iter().map(EntityTypeName::as_str).collect()
    }

    /// Clause 5.6.12.4 adds a type only when the Fragment includes one: a
    /// Fragment naming none is the Temporal Evolution under the types it is
    /// recorded under.
    #[test]
    fn a_fragment_naming_no_type_takes_the_recorded_types() {
        let recorded = vec!["https://example.org/Vehicle".parse().expect("a valid type")];
        let addressed = written_of(&[]).addressed_to(&identifier("urn:ngsi-ld:Vehicle:1"), recorded);
        pretty_assertions::assert_eq!(types_of(&addressed), ["https://example.org/Vehicle"]);
    }

    /// Clause 5.6.12.4: a Fragment naming types states the types it adds, and
    /// the store adds the ones the Temporal Evolution lacks to its list, so
    /// the recorded types are not copied into what the write states.
    #[test]
    fn a_fragment_naming_types_states_its_own_types() {
        let recorded = vec!["https://example.org/Vehicle".parse().expect("a valid type")];
        let addressed = written_of(&["https://example.org/Car"]).addressed_to(&identifier("urn:ngsi-ld:Vehicle:1"), recorded);
        pretty_assertions::assert_eq!(types_of(&addressed), ["https://example.org/Car"]);
    }

    /// A Temporal Evolution is held to the attribute nesting a Core write is:
    /// an attribute at the limit is admitted, one level deeper is refused
    /// naming the attribute.
    #[test]
    fn an_evolution_is_admitted_at_the_nesting_limit_and_refused_one_level_past_it() {
        const NESTED_ATTRIBUTE: &str = "https://example.org/nested";

        // A Property of a scalar carrying a chain of `levels` sub-attributes,
        // each a Property of a scalar, as the stored expanded document holds it.
        let evolution = |levels| {
            let property = |inner: Option<Value>| {
                let mut instance = json!({
                    "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                    "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
                    "https://uri.etsi.org/ngsi-ld/createdAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2026-01-01T00:00:00Z"}],
                    "https://uri.etsi.org/ngsi-ld/modifiedAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2026-01-01T00:00:00Z"}],
                });
                if let (Some(members), Some(inner)) = (instance.as_object_mut(), inner) {
                    members.insert("https://example.org/inner".to_owned(), json!([inner]));
                }
                instance
            };
            let chain = (0..levels).fold(property(None), |inner, _| property(Some(inner)));
            let stored = json!([{"@id": "urn:ngsi-ld:NestingProbe:1", "@type": ["https://example.org/NestingProbe"], NESTED_ATTRIBUTE: [chain]}]);
            let document = EntityDocument::from_slice(&serde_json::to_vec(&stored).expect("JSON")).expect("a written document");
            WrittenEvolution::new(Arc::new(document), None)
        };
        let budget = payload_budget(&RequestLimitsConfig::default());
        assert!(evolution(32).admit_budget(budget).is_ok());
        let refused = evolution(33).admit_budget(budget).expect_err("33 levels pass the default limit of 32");
        let TemporalError::AttributeNesting(exceeded) = refused else {
            panic!("refused for its nesting: {refused}");
        };
        pretty_assertions::assert_eq!(exceeded.attribute.as_str(), NESTED_ATTRIBUTE);
    }

    /// Expand `payload` as a client's Temporal Evolution, against the core
    /// context and a `settings` term coerced to `@json`.
    async fn expanded(payload: Value) -> Result<WrittenEvolution, TemporalError> {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("build engine");
        let context = ContextSpec::from_body(
            JsonLiteral::admit(&json!({"settings": {"@id": "https://example.org/settings", "@type": "@json"}}).to_string()).expect("an admissible literal"),
        );
        let expansion = EntityExpansion::prepare(&engine, &context).await.expect("the context resolves");
        let written = WrittenTemporalEntity::from_request_text(payload.to_string().as_bytes(), ContextReading::keyword_spellings())
            .expect("a written Temporal Evolution");
        WrittenEvolution::expand(&expansion, written).await
    }

    /// One observation of `config` written as `instance`, observed once.
    fn evolution_of(instance: &Value) -> Value {
        let mut observed = instance.clone();
        observed["observedAt"] = json!("2024-01-01T00:00:00Z");
        json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "config": [observed]})
    }

    /// Clause 5.5.4 excepts the temporal evolution from the second NGSI-LD
    /// Null prohibition only: the Null under a key of a structured value, of a
    /// JSON literal in a value, and of a `JsonProperty` is refused.
    #[tokio::test]
    async fn a_temporal_evolution_refuses_a_null_under_a_key_of_a_property_value() {
        for instance in [
            json!({"type": "Property", "value": {"street": "x", "country": "urn:ngsi-ld:null"}}),
            json!({"type": "Property", "value": {"settings": {"a": 1, "b": "urn:ngsi-ld:null"}}}),
            json!({"type": "Property", "value": {"@type": "@json", "@value": {"b": "urn:ngsi-ld:null"}}}),
            json!({"type": "JsonProperty", "json": {"b": "urn:ngsi-ld:null"}}),
        ] {
            let refused = expanded(evolution_of(&instance))
                .await
                .expect_err("the third prohibition has no temporal exception");
            assert!(
                matches!(
                    refused,
                    TemporalError::Validation(TemporalValidationReason::Representation(EntityRefusal::NestedNullValue { .. }))
                ),
                "{instance}: {refused:?}"
            );
        }
    }

    /// Clauses 4.5.7 and 5.5.4: an instance whose value is the Null and which
    /// carries `deletedAt` is the representation of a deletion, written beside
    /// an observation of the same attribute.
    #[tokio::test]
    async fn a_temporal_evolution_admits_a_deleted_instance_dated_beside_an_observation() {
        let observed = json!({"type": "Property", "value": 1, "observedAt": "2024-01-01T00:00:00Z"});
        let deleted = json!({"type": "Property", "value": "urn:ngsi-ld:null", "observedAt": "2024-01-02T00:00:00Z", "deletedAt": "2024-01-02T00:00:00Z"});
        let payload = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "config": [observed, deleted]});
        let evolution = expanded(payload).await.expect("a dated deletion is the temporal representation of a deletion");
        fold_into_current_state(None, evolution.document(), chrono::Utc::now()).expect("the fold takes the deletion beside the observation");
    }

    /// Clause 4.5.6: a reified Scope carrying only the four Temporal
    /// Properties beside its `value` is the history the write dates.
    #[tokio::test]
    async fn a_reified_scope_carrying_the_four_temporal_properties_is_admitted() {
        let payload = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "scope": admitted_scope()});
        let evolution = expanded(payload)
            .await
            .expect("the four Temporal Properties are a reified Scope's sub-Properties");
        let scope = evolution.reified_scope.expect("the Scope history is carried");
        pretty_assertions::assert_eq!(scope.attributes()[expanded_properties::SCOPE.as_str()].instances().len(), 2);
    }

    /// The deleted instance clause 5.5.4 lets a temporal evolution represent
    /// is still written.
    #[tokio::test]
    async fn a_temporal_evolution_admits_a_deleted_instance() {
        expanded(evolution_of(&json!({"type": "Property", "value": "urn:ngsi-ld:null"})))
            .await
            .expect("a deleted instance is the temporal representation of a deletion");
    }

    /// Clause 5.2.20 makes the Temporal Evolution of an Entity the datatype of
    /// clause 5.2.4, whose Table 5.2.4-1 types the three geospatial members as
    /// `GeoProperty`.
    #[tokio::test]
    async fn a_temporal_evolution_refuses_a_geospatial_member_written_as_a_property() {
        for member in ["location", "observationSpace", "operationSpace"] {
            let observed = json!({"type": "Property", "value": "somewhere", "observedAt": "2024-01-01T00:00:00Z"});
            let payload = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", member: [observed]});
            let refused = expanded(payload).await.expect_err("a Property is no GeoProperty");
            assert!(
                matches!(
                    refused,
                    TemporalError::Validation(TemporalValidationReason::Representation(EntityRefusal::NotAGeoProperty { .. }))
                ),
                "{member}: {refused:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_temporal_evolution_admits_observed_and_deleted_geo_property_locations() {
        let observed = json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.5]}, "observedAt": "2024-01-01T00:00:00Z"});
        let deleted = json!({"type": "GeoProperty", "value": "urn:ngsi-ld:null", "observedAt": "2024-01-02T00:00:00Z", "deletedAt": "2024-01-02T00:00:00Z"});
        let payload = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "location": [observed, deleted]});
        expanded(payload).await.expect("GeoProperty observations and a dated deletion");
    }
}
