//! The Temporal Evolution a client writes (clauses 5.6.11 and 5.6.12), from
//! its compact form to the written documents the write records.
//!
//! Two shapes a Temporal Evolution may take have no place in an entity
//! document and are therefore read before the typed expansion runs. A plural
//! member (`values`, `objects`, `languageMaps`, ...) carries several
//! observations under one envelope, each inheriting the envelope's members
//! unless it restates them; it is unfolded into the instances it stands for,
//! within the payload budget (`crate::plural_member_unfold`).
//! A reified Scope (clause 4.5.6: "a temporal representation of a Property")
//! dates the Scope paths it names; it is expanded as the attribute instances
//! it is and recorded beside the entity document, keyed by the scope IRI as the
//! temporal representation keys it.

use crate::{
    error::TemporalError,
    plural_member_unfold::unfold_plural_members,
    reified_scope::scope_observations,
    temporal_validation_reason::TemporalValidationReason,
    unit_code_admission::admit_document_unit_codes,
};
use andromeda_context_commit::history_record::HistoryRecord;
use andromeda_entity::document_census::admit_entity_document;
use andromeda_ngsi_ld_entity::{
    admission::attribute_nesting_admission::admit_document_nesting,
    document::{EntityDocument, EntityDocumentParts, attribute_member::AttributeMember},
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_expansion::{entity_expansion::EntityExpansion, normalization::Normalization};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_representation::payload_budget::EntityPayloadBudget;
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, expanded::properties as expanded_properties};
use indexmap::IndexMap;
use serde_json::Value;
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

    /// Expand the compact Temporal Evolution `payload` under `expansion`, its
    /// plural members unfolded within `budget`.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::PayloadBudget`] when the observations of its
    /// plural members inherit more values than `budget` admits,
    /// [`TemporalError::JsonLd`] when the context does not resolve or the
    /// processor refuses the payload, and [`TemporalError::Validation`] when
    /// the payload is no Temporal Evolution the model admits or puts a
    /// `unitCode` on a family defined without one.
    pub(crate) async fn expand(expansion: &EntityExpansion<'_>, payload: Value, budget: EntityPayloadBudget) -> Result<WrittenEvolution, TemporalError> {
        let EvolutionDocuments { document, reified_scope } = read_evolution_document(expansion, payload, budget).await?;
        admit_document_unit_codes(&document)?;
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
        let id = Some(entity_id.clone()); // clone: the document owns the identifier the request path names
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

/// One compact Temporal Evolution, read as the documents it stands for.
#[derive(Debug)]
pub(crate) struct EvolutionDocuments {
    /// The instances the evolution carries, with its identifier, types and any
    /// non-reified Scope.
    pub(crate) document: EntityDocument,
    /// The reified Scope instances the evolution dates, keyed by the scope
    /// IRI; absent when it dates none.
    pub(crate) reified_scope: Option<EntityDocument>,
}

/// Read the compact Temporal Evolution `payload` under `expansion`: its plural
/// members unfolded within `budget` (clause 4.5.9), its reified Scope taken out
/// and expanded on its own (clause 4.5.6), and the rest expanded as the entity
/// document it is.
///
/// A Temporal Evolution a client writes and one a Context Source answers with
/// are the same representation, so both are read through here.
///
/// # Errors
///
/// Returns [`TemporalError::PayloadBudget`] when the observations of its plural
/// members inherit more values than `budget` admits, [`TemporalError::JsonLd`]
/// when the context does not resolve or the processor refuses the payload, and
/// [`TemporalError::Validation`] when the payload is no Temporal Evolution the
/// model admits.
pub(crate) async fn read_evolution_document(
    expansion: &EntityExpansion<'_>,
    mut payload: Value,
    budget: EntityPayloadBudget,
) -> Result<EvolutionDocuments, TemporalError> {
    unfold_plural_members(&mut payload, budget.json_values)?;
    let reified = take_reified_scope(&mut payload);
    let document = expansion.entity_fragment(payload, Normalization::Entity).await?;
    let reified_scope = match reified {
        Some(instances) => Some(expand_reified_scope(expansion, instances).await?),
        None => None,
    };
    Ok(EvolutionDocuments { document, reified_scope })
}

/// Expand each reified Scope instance as the attribute instance it is, into a
/// document holding them under the scope IRI alone, refusing an instance that
/// states no Scope (`crate::reified_scope`).
async fn expand_reified_scope(expansion: &EntityExpansion<'_>, instances: Vec<Value>) -> Result<EntityDocument, TemporalError> {
    let written_name = compact_properties::SCOPE
        .parse::<AttributeName>()
        .map_err(|source| TemporalError::from(TemporalValidationReason::AttributeName { source }))?;
    let mut expanded = Vec::with_capacity(instances.len());
    for instance in instances {
        let instance = expansion.attribute_fragment(&written_name, instance).await?.into_parts().1;
        scope_observations(instance.value()).map_err(|refusal| TemporalError::from(EntityRefusal::Scope(refusal)))?;
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

#[cfg(test)]
mod tests {
    use crate::written_evolution::{WrittenEvolution, take_reified_scope};
    use andromeda_ngsi_ld_entity::document::EntityDocument;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use indexmap::IndexMap;
    use serde_json::json;
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

    /// A Temporal Evolution is held to the attribute nesting a Core write is:
    /// an attribute at the limit is admitted, one level deeper is refused
    /// naming the attribute.
    #[test]
    fn an_evolution_is_admitted_at_the_nesting_limit_and_refused_one_level_past_it() {
        use crate::error::TemporalError;
        use andromeda_entity::config::EntityConfig;
        use andromeda_test_fixtures::nesting_fixture::{NESTED_ATTRIBUTE, NestingShape, nested_instance, stored_entity};

        let evolution = |levels| {
            let stored = stored_entity("urn:ngsi-ld:NestingProbe:1", nested_instance(NestingShape::SubAttributeChain, levels));
            let document = EntityDocument::from_slice(&serde_json::to_vec(&stored).expect("JSON")).expect("a written document");
            WrittenEvolution::new(Arc::new(document), None)
        };
        let budget = EntityConfig::default().payload_budget();
        assert!(evolution(32).admit_budget(budget).is_ok());
        let refused = evolution(33).admit_budget(budget).expect_err("33 levels pass the default limit of 32");
        let TemporalError::AttributeNesting(exceeded) = refused else {
            panic!("refused for its nesting: {refused}");
        };
        pretty_assertions::assert_eq!(exceeded.attribute.as_str(), NESTED_ATTRIBUTE);
    }
}
