//! Temporal mutation policy.

use crate::{
    error::TemporalError,
    port::mutator::TemporalDeleteScope,
    service::{CreateOrUpdateResult, TemporalService, iri_short_name},
    temporal_not_found_reason::TemporalNotFoundReason,
    temporal_validation_reason::TemporalValidationReason,
    unit_code_admission::validate_entity_unit_code_admission,
};
use andromeda_context_commit::{atomic_write::AtomicWrite, entity_mutation::EntityMutation, entity_record::EntityRecord, history_record::HistoryRecord};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use andromeda_ngsi_ld_representation::normalization::normalize_entity_attributes;
use andromeda_telemetry::temporal::{self, TemporalOperation};
use serde_json::Value;
use std::sync::Arc;

impl TemporalService {
    /// Create a temporal entity or append the supplied observations when it already exists.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::Validation`] when the payload is not a JSON
    /// object, carries no `@id` or no `@type` (clause 4.5.1), names an
    /// unparsable identifier, or puts a `unitCode` on an attribute family
    /// clause 4.5.2 defines without one; [`TemporalError::JsonLd`] when the
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
    async fn create_or_update_inner(&self, target: &ContextTarget, mut payload: Value, context: &ContextSpec) -> Result<CreateOrUpdateResult, TemporalError> {
        normalize_entity_attributes(&mut payload);
        let expanded = self.engine.expand(payload, context).await?;
        validate_entity_unit_code_admission(&expanded)?;
        let entity_id = {
            let entity_obj = expanded
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_object())
                .ok_or_else(|| TemporalError::Validation(TemporalValidationReason::EntityNotObject))?;

            let entity_id = entity_obj
                .get("@id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| TemporalError::Validation(TemporalValidationReason::EntityMissingId))?;

            let entity_id = entity_id
                .parse::<Uri>()
                .map_err(|source| TemporalError::Validation(TemporalValidationReason::EntityId { source }))?;
            if entity_obj.get("@type").and_then(|v| v.as_array()).is_none_or(Vec::is_empty) {
                return Err(TemporalError::Validation(TemporalValidationReason::EntityMissingType));
            }
            entity_id
        };

        // Per the specification, temporal entities can be created independently
        // of the entity table, so the entity is not required to exist first.
        let exists = self.reader.entity_exists(target, &entity_id).await?;

        let payload = Arc::new(expanded);

        if exists {
            self.add_attrs_internal(target, &entity_id, payload).await?;
            Ok(CreateOrUpdateResult::Updated)
        } else {
            // The base entity row is a placeholder carrying the expanded
            // payload, so the stored JSON has the `@type` every entity read
            // expects. No pre-flight existence check: the statement's conflict
            // clause is the check, and it does not lose a race to a concurrent
            // temporal write for the same new identifier.
            let write = AtomicWrite::builder()
                .entity_id(Arc::new(entity_id.clone())) // clone: the write owns a shared validated identifier.
                // Unconditional: two concurrent temporal writes for one new
                // identifier are both legitimate, and neither should fail
                // because the other materialised the row first.
                .entity(EntityMutation::Insert(EntityRecord::from_expanded(Arc::clone(&payload)))) // clone: Arc ref-count bump
                .history(vec![HistoryRecord::Observations(payload)])
                .build();
            self.writes.commit(target, vec![write]).await?;

            Ok(CreateOrUpdateResult::Created(entity_id))
        }
    }

    /// Add temporal attributes to an existing temporal entity.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::NotFound`] when the entity has no Temporal
    /// Evolution, [`TemporalError::Validation`] when the payload puts a
    /// `unitCode` on an attribute family clause 4.5.2 defines without one,
    /// [`TemporalError::JsonLd`] when the payload cannot be expanded against
    /// `context`, and [`TemporalError::Commit`] or
    /// [`TemporalError::Persistence`] when the append cannot be landed.
    pub async fn add_attrs(&self, target: &ContextTarget, entity_id: &Uri, payload: Value, context: &ContextSpec) -> Result<(), TemporalError> {
        let timer = temporal::start(TemporalOperation::AddAttributes);
        let result = self.add_attrs_inner(target, entity_id, payload, context).await;
        timer.finish_result(&result);
        result
    }

    /// The measured body of [`TemporalService::add_attrs`].
    async fn add_attrs_inner(&self, target: &ContextTarget, entity_id: &Uri, mut payload: Value, context: &ContextSpec) -> Result<(), TemporalError> {
        if !self.reader.entity_exists(target, entity_id).await? {
            return Err(TemporalError::NotFound(TemporalNotFoundReason::Entity { id: entity_id.clone() }));
        }

        normalize_entity_attributes(&mut payload);
        let expanded = self.engine.expand(payload, context).await?;
        validate_entity_unit_code_admission(&expanded)?;
        self.add_attrs_internal(target, entity_id, Arc::new(expanded)).await?;
        Ok(())
    }

    /// Append every observation in `expanded` to the entity's Temporal Evolution
    /// in one transaction.
    ///
    /// No entity placeholder is written here: the caller has already established
    /// that the entity has `temporal_entity_attributes` rows, and their foreign
    /// key to `entities(id)` cascades on delete, so the base row is necessarily
    /// present.
    ///
    /// A database failure fails the whole request. NGSI-LD 6.20.3.1 defines only
    /// 204/400/404 for `POST /temporal/entities/{id}/attrs`, and 204 means "all
    /// the Attributes were added successfully" -- there is no response code that
    /// can report a subset having been stored.
    async fn add_attrs_internal(&self, target: &ContextTarget, entity_id: &Uri, expanded: Arc<Value>) -> Result<(), TemporalError> {
        let write = AtomicWrite::builder()
            .entity_id(Arc::new(entity_id.clone())) // clone: the write owns a shared validated identifier.
            .entity(EntityMutation::None)
            .history(vec![HistoryRecord::Observations(expanded)])
            .build();
        self.writes.commit(target, vec![write]).await?;
        Ok(())
    }

    /// Resolve a compact attribute name while retaining compatibility with already-expanded names.
    pub(crate) async fn resolve_attr_iri_for_entity(
        &self,
        target: &ContextTarget,
        entity_id: &Uri,
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
    pub async fn delete(&self, target: &ContextTarget, entity_id: &Uri) -> Result<(), TemporalError> {
        let timer = temporal::start(TemporalOperation::Delete);
        let result = self.delete_inner(target, entity_id).await;
        timer.finish_result(&result);
        result
    }

    /// The measured body of [`TemporalService::delete`].
    async fn delete_inner(&self, target: &ContextTarget, entity_id: &Uri) -> Result<(), TemporalError> {
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
        entity_id: &Uri,
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
        entity_id: &Uri,
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
