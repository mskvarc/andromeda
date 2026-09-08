//! Temporal mutation policy.

use crate::{
    error::TemporalError,
    port::mutator::TemporalDeleteScope,
    service::{CreateOrUpdateResult, TemporalService, iri_short_name},
    unit_code_admission::{validate_entity_unit_code_admission, validate_instance_unit_code_admission},
};
use andromeda_context_commit::{atomic_write::AtomicWrite, entity_mutation::EntityMutation, entity_record::EntityRecord, history_record::HistoryRecord};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, instance_family::InstanceFamily, instance_identity::InstanceIdentity, uri::Uri};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use andromeda_ngsi_ld_representation::normalization::normalize_entity_attributes;
use serde_json::Value;
use std::sync::Arc;

impl TemporalService {
    /// Create a temporal entity or append the supplied observations when it already exists.
    pub async fn create_or_update(&self, target: &ContextTarget, mut payload: Value, context: &ContextSpec) -> Result<CreateOrUpdateResult, TemporalError> {
        normalize_entity_attributes(&mut payload);
        let expanded = self.engine.expand(payload, context).await?;
        validate_entity_unit_code_admission(&expanded)?;
        let entity_id = {
            let entity_obj = expanded
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_object())
                .ok_or_else(|| TemporalError::validation("Entity must be a JSON object"))?;

            let entity_id = entity_obj
                .get("@id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| TemporalError::validation("Entity missing @id"))?;

            let entity_id = entity_id
                .parse::<Uri>()
                .map_err(|error| TemporalError::validation(format!("Invalid entity id: {error}")))?;
            if entity_obj.get("@type").and_then(|v| v.as_array()).is_none_or(std::vec::Vec::is_empty) {
                return Err(TemporalError::validation("Entity missing @type"));
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
    pub async fn add_attrs(&self, target: &ContextTarget, entity_id: &Uri, mut payload: Value, context: &ContextSpec) -> Result<(), TemporalError> {
        if !self.reader.entity_exists(target, entity_id).await? {
            return Err(TemporalError::not_found(entity_id.to_string()));
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
    async fn resolve_attr_iri_for_entity(
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
            .map_err(|error| TemporalError::validation(format!("Expanded attribute name is invalid: {error}")))?;

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
    pub async fn delete(&self, target: &ContextTarget, entity_id: &Uri) -> Result<(), TemporalError> {
        self.mutation_barrier.synchronize(target, entity_id).await?;
        if !self.mutator.entity_exists(target, entity_id).await? {
            return Err(TemporalError::not_found(entity_id.to_string()));
        }
        self.mutator.delete_all_for_entity(target, entity_id).await?;
        Ok(())
    }

    /// Delete a specific temporal attribute (optionally filtered by dataset ID).
    pub async fn delete_attr(
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
            return Err(TemporalError::not_found(entity_id.to_string()));
        }

        if !self.mutator.attribute_exists(target, entity_id, &attr_iri).await? {
            return Err(TemporalError::not_found(format!("Attribute {attr_iri} not found on entity {entity_id}")));
        }

        let dataset_uri = match dataset_id {
            Some(DatasetSelector::Id(uri)) => Some(uri),
            Some(DatasetSelector::Default) | None => None,
        };
        let deleted = self.mutator.delete_attribute(target, entity_id, &attr_iri, dataset_uri, scope).await?;
        if deleted == 0 {
            let missing_detail = match dataset_id {
                Some(ds_id) => format!("Attribute {attr_iri} with datasetId {ds_id} not found on entity {entity_id}"),
                None => format!("Attribute {attr_iri} default instance not found on entity {entity_id}"),
            };
            return Err(TemporalError::not_found(missing_detail));
        }
        Ok(())
    }

    /// Replace an existing attribute instance with new data.
    pub async fn modify_attr_instance(
        &self,
        target: &ContextTarget,
        entity_id: &Uri,
        attr_id: &AttributeName,
        instance: &InstanceIdentity,
        payload: Value,
        context: &ContextSpec,
    ) -> Result<(), TemporalError> {
        self.mutation_barrier.synchronize(target, entity_id).await?;
        let attr_iri = self.resolve_attr_iri_for_entity(target, entity_id, attr_id, context).await?;

        if !self.mutator.entity_exists(target, entity_id).await? {
            return Err(TemporalError::not_found(format!("Entity {entity_id} not found")));
        }

        if !self.mutator.attribute_exists(target, entity_id, &attr_iri).await? {
            return Err(TemporalError::not_found(format!("Attribute {attr_iri} not found on entity {entity_id}")));
        }

        if !self.mutator.instance_exists(target, instance, entity_id, &attr_iri).await? {
            return Err(TemporalError::not_found(format!(
                "Attribute instance {} not found",
                instance.rendered(InstanceFamily::Attribute)
            )));
        }

        let expanded = self.engine.expand(payload, context).await?;
        let instance_value = expanded.as_array().and_then(|arr| arr.first()).cloned().unwrap_or(expanded); // clone: extracting first element from array requires owned value; original consumed by unwrap_or
        validate_instance_unit_code_admission(attr_iri.as_str(), &instance_value)?;

        self.mutator.update_instance(target, instance, entity_id, &attr_iri, &instance_value).await
    }

    /// Delete a specific attribute instance by its instance ID.
    pub async fn delete_attr_instance(
        &self,
        target: &ContextTarget,
        entity_id: &Uri,
        attr_id: &AttributeName,
        instance: &InstanceIdentity,
        context: &ContextSpec,
    ) -> Result<(), TemporalError> {
        self.mutation_barrier.synchronize(target, entity_id).await?;
        let attr_iri = self.resolve_attr_iri_for_entity(target, entity_id, attr_id, context).await?;

        if !self.mutator.entity_exists(target, entity_id).await? {
            return Err(TemporalError::not_found(format!("Entity {entity_id} not found")));
        }

        if !self.mutator.attribute_exists(target, entity_id, &attr_iri).await? {
            return Err(TemporalError::not_found(format!("Attribute {attr_iri} not found on entity {entity_id}")));
        }

        if !self.mutator.instance_exists(target, instance, entity_id, &attr_iri).await? {
            return Err(TemporalError::not_found(format!(
                "Attribute instance {} not found",
                instance.rendered(InstanceFamily::Attribute)
            )));
        }

        self.mutator.delete_instance(target, instance, entity_id, &attr_iri).await?;
        Ok(())
    }
}
