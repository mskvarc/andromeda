//! Temporal mutation policy.

use crate::{
    error::TemporalError,
    port::mutator::TemporalDeleteScope,
    service::{CreateOrUpdateResult, TemporalService, iri_short_name},
    temporal_not_found_reason::TemporalNotFoundReason,
    temporal_validation_reason::TemporalValidationReason,
    written_evolution::WrittenEvolution,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_expansion::entity_expansion::EntityExpansion;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use andromeda_telemetry::temporal::{self, TemporalOperation};
use serde_json::Value;
use std::slice::from_ref;

impl TemporalService {
    /// Create a temporal entity or append the supplied observations when it already exists.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Validation`] when the payload is no Temporal
    /// Evolution the model admits, carries no `id` or no `type` (clause
    /// 4.5.1), puts a `unitCode` on an attribute family clause 4.5.2 defines
    /// without one, or carries a `GeoProperty` whose geometry is unusable
    /// (clause 4.7); [`TemporalError::PayloadBudget`] when it exceeds the
    /// budget an entity is admitted under; [`TemporalError::JsonLd`] when the
    /// payload cannot be expanded against `context`; and
    /// [`TemporalError::Commit`] or [`TemporalError::Persistence`] when the
    /// atomic write cannot be landed.
    pub async fn create_or_update(&self, target: &ContextTarget, payload: Value, context: &ContextSpec) -> Result<CreateOrUpdateResult, TemporalError> {
        let timer = temporal::start(TemporalOperation::CreateOrUpdate);
        let result = self.create_or_update_inner(target, payload, context).await;
        timer.finish_result(&result);
        result
    }

    /// The measured body of [`TemporalService::create_or_update`].
    async fn create_or_update_inner(&self, target: &ContextTarget, payload: Value, context: &ContextSpec) -> Result<CreateOrUpdateResult, TemporalError> {
        let expansion = EntityExpansion::prepare(&self.engine, context).await?;
        let evolution = WrittenEvolution::expand(&expansion, payload).await?;
        evolution.admit_budget(self.limits.payload_budget)?;
        let document = evolution.document();
        let entity_id = document
            .id()
            .cloned() // clone: the identifier outlives the evolution it is read from
            .ok_or(TemporalError::Validation(TemporalValidationReason::EntityMissingId))?;
        if document.types().is_empty() {
            return Err(TemporalError::Validation(TemporalValidationReason::EntityMissingType));
        }

        // Clause 5.6.11.4: an existing Temporal Evolution gets the instances
        // added "as mandated by clause 5.6.12"; otherwise "implementations
        // shall create the provided Temporal Evolution of an Entity". Both are
        // the same write -- every instance to history, the standing ones to
        // current state -- and the answer differs only in what it reports.
        let exists = self.reader.entity_exists(target, &entity_id).await?;
        self.commit_evolution(target, &entity_id, &evolution).await?;
        if exists {
            Ok(CreateOrUpdateResult::Updated)
        } else {
            Ok(CreateOrUpdateResult::Created(entity_id))
        }
    }

    /// Add temporal attributes to an existing temporal entity.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::NotFound`] when the entity has no Temporal
    /// Evolution, [`TemporalError::Validation`] when the payload is no
    /// fragment the model admits, puts a `unitCode` on an attribute family
    /// clause 4.5.2 defines without one or carries a `GeoProperty` whose
    /// geometry is unusable (clause 4.7), [`TemporalError::PayloadBudget`]
    /// when it exceeds the budget an entity is admitted under,
    /// [`TemporalError::JsonLd`] when the payload cannot be expanded against
    /// `context`, and [`TemporalError::Commit`] or
    /// [`TemporalError::Persistence`] when the append cannot be landed.
    pub async fn add_attrs(&self, target: &ContextTarget, entity_id: &Identifier, payload: Value, context: &ContextSpec) -> Result<(), TemporalError> {
        let timer = temporal::start(TemporalOperation::AddAttributes);
        let result = self.add_attrs_inner(target, entity_id, payload, context).await;
        timer.finish_result(&result);
        result
    }

    /// The measured body of [`TemporalService::add_attrs`].
    async fn add_attrs_inner(&self, target: &ContextTarget, entity_id: &Identifier, payload: Value, context: &ContextSpec) -> Result<(), TemporalError> {
        if !self.reader.entity_exists(target, entity_id).await? {
            return Err(TemporalError::NotFound(TemporalNotFoundReason::Entity { id: entity_id.clone() }));
        }

        let expansion = EntityExpansion::prepare(&self.engine, context).await?;
        let evolution = WrittenEvolution::expand(&expansion, payload).await?;
        evolution.admit_budget(self.limits.payload_budget)?;
        // Clause 5.6.12 addresses the Temporal Evolution by the request path, and
        // clause 4.5.6 makes it the Entity with the types its evolution records:
        // the fragment names neither, yet the instances it adds may have to
        // bring current state back into being.
        let recorded_types = self
            .reader
            .entity_types(target, from_ref(entity_id))
            .await?
            .remove(entity_id)
            .unwrap_or_default();
        let evolution = evolution.addressed_to(entity_id, recorded_types);
        self.commit_evolution(target, entity_id, &evolution).await?;
        Ok(())
    }

    /// Resolve a compact attribute name while retaining compatibility with already-expanded names.
    pub(crate) async fn resolve_attr_iri_for_entity(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        attr_id: &AttributeName,
        context: &ContextSpec,
    ) -> Result<AttributeName, TemporalError> {
        let prepared_context = self.engine.prepare_term_context(context);
        let expanded_attr = self
            .engine
            .expand_term(attr_id.as_str(), &prepared_context)
            .await?
            .to_string()
            .parse::<AttributeName>()
            .map_err(|source| TemporalError::Validation(TemporalValidationReason::AttributeName { source }))?;

        if self.mutator.attribute_exists(target, entity_id, &expanded_attr).await? {
            return Ok(expanded_attr);
        }

        if expanded_attr != *attr_id && self.mutator.attribute_exists(target, entity_id, attr_id).await? {
            return Ok(attr_id.clone()); // clone: the resolved name is returned after borrowing the request value.
        }

        let attr_names = self.mutator.attribute_names_for_entity(target, entity_id).await?;
        let mut matches = attr_names
            .into_iter()
            .filter(|name| iri_short_name(name.as_str()).is_some_and(|short| short == attr_id.as_str()))
            .collect::<Vec<_>>();

        if matches.len() == 1
            && let Some(name) = matches.pop()
        {
            return Ok(name);
        }

        Ok(expanded_attr)
    }

    /// Delete all temporal data for an entity.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::NotFound`] when the entity has no Temporal
    /// Evolution, and [`TemporalError::Persistence`] when the delivery
    /// barrier cannot drain the entity's pending history or the deletion
    /// cannot be applied.
    pub async fn delete(&self, target: &ContextTarget, entity_id: &Identifier) -> Result<(), TemporalError> {
        let timer = temporal::start(TemporalOperation::Delete);
        let result = self.delete_inner(target, entity_id).await;
        timer.finish_result(&result);
        result
    }

    /// The measured body of [`TemporalService::delete`].
    async fn delete_inner(&self, target: &ContextTarget, entity_id: &Identifier) -> Result<(), TemporalError> {
        self.mutation_barrier.synchronize(target, entity_id).await?;
        if !self.mutator.entity_exists(target, entity_id).await? {
            return Err(TemporalError::NotFound(TemporalNotFoundReason::Entity { id: entity_id.clone() }));
        }
        self.mutator.delete_all_for_entity(target, entity_id).await?;
        Ok(())
    }

    /// Delete a specific temporal attribute (optionally filtered by dataset ID).
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::NotFound`] when the entity has no Temporal
    /// Evolution, when it carries no such attribute, or when the named
    /// `datasetId` -- or the default instance -- matches nothing;
    /// [`TemporalError::Validation`] when the attribute name does not expand
    /// to a valid one; and [`TemporalError::Persistence`] when the delivery
    /// barrier cannot drain the entity's pending history or the deletion
    /// cannot be applied.
    pub async fn delete_attr(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        attr_id: &AttributeName,
        dataset_id: Option<&DatasetSelector>,
        scope: TemporalDeleteScope,
        context: &ContextSpec,
    ) -> Result<(), TemporalError> {
        let timer = temporal::start(TemporalOperation::DeleteAttribute);
        let result = self.delete_attr_inner(target, entity_id, attr_id, dataset_id, scope, context).await;
        timer.finish_result(&result);
        result
    }

    /// The measured body of [`TemporalService::delete_attr`].
    async fn delete_attr_inner(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        attr_id: &AttributeName,
        dataset_id: Option<&DatasetSelector>,
        scope: TemporalDeleteScope,
        context: &ContextSpec,
    ) -> Result<(), TemporalError> {
        self.mutation_barrier.synchronize(target, entity_id).await?;
        let attr_iri = self.resolve_attr_iri_for_entity(target, entity_id, attr_id, context).await?;

        if !self.mutator.entity_exists(target, entity_id).await? {
            return Err(TemporalError::NotFound(TemporalNotFoundReason::Entity { id: entity_id.clone() }));
        }

        if !self.mutator.attribute_exists(target, entity_id, &attr_iri).await? {
            return Err(TemporalError::NotFound(TemporalNotFoundReason::AttributeOnEntity {
                attribute: attr_iri.clone(),
                entity: entity_id.clone(),
            }));
        }

        let dataset_uri = match dataset_id {
            Some(DatasetSelector::Id(uri)) => Some(uri),
            Some(DatasetSelector::Default) | None => None,
        };
        let deleted = self.mutator.delete_attribute(target, entity_id, &attr_iri, dataset_uri, scope).await?;
        if deleted == 0 {
            // clone: the error owns the borrowed request attribute and identifier.
            let missing = match dataset_id {
                Some(DatasetSelector::Id(ds_id)) => TemporalNotFoundReason::AttributeDatasetOnEntity {
                    attribute: attr_iri.clone(),
                    dataset_id: Box::new(ds_id.clone()),
                    entity: entity_id.clone(),
                },
                Some(DatasetSelector::Default) | None => TemporalNotFoundReason::AttributeDefaultInstanceOnEntity {
                    attribute: attr_iri.clone(),
                    entity: entity_id.clone(),
                },
            };
            return Err(TemporalError::NotFound(missing));
        }
        Ok(())
    }
}
