//! Temporal attribute-instance mutation policy.
//!
//! The instance-level operations of clauses 5.6.14 and 5.6.15: replacing one
//! recorded attribute instance and deleting one recorded attribute instance,
//! each addressed by its `instanceId`. Both hang off [`TemporalService`] and
//! share the attribute-name resolution and the mutation barrier with the
//! entity- and attribute-level operations in [`crate::write`].

use crate::{
    error::TemporalError,
    service::TemporalService,
    temporal_not_found_reason::TemporalNotFoundReason,
    unit_code_admission::validate_instance_unit_code_admission,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, instance_family::InstanceFamily, instance_identity::InstanceIdentity, uri::Uri};
use andromeda_telemetry::temporal::{self, TemporalOperation};
use serde_json::Value;

impl TemporalService {
    /// Replace an existing attribute instance with new data.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::NotFound`] when the entity, the attribute or
    /// the named instance does not exist; [`TemporalError::Validation`] when
    /// the attribute name does not expand to a valid one or the replacement
    /// puts a `unitCode` on an attribute family clause 4.5.2 defines without
    /// one; [`TemporalError::JsonLd`] when the payload cannot be expanded
    /// against `context`; and [`TemporalError::Persistence`] when the
    /// delivery barrier cannot drain the entity's pending history or the
    /// update cannot be applied.
    pub async fn modify_attr_instance(
        &self,
        target: &ContextTarget,
        entity_id: &Uri,
        attr_id: &AttributeName,
        instance: &InstanceIdentity,
        payload: Value,
        context: &ContextSpec,
    ) -> Result<(), TemporalError> {
        let timer = temporal::start(TemporalOperation::ModifyAttributeInstance);
        let result = self.modify_attr_instance_inner(target, entity_id, attr_id, instance, payload, context).await;
        timer.finish_result(&result);
        result
    }

    /// The measured body of [`TemporalService::modify_attr_instance`].
    async fn modify_attr_instance_inner(
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
            return Err(TemporalError::NotFound(TemporalNotFoundReason::Entity { id: entity_id.clone() }));
        }

        if !self.mutator.attribute_exists(target, entity_id, &attr_iri).await? {
            return Err(TemporalError::NotFound(TemporalNotFoundReason::AttributeOnEntity {
                attribute: attr_iri.clone(),
                entity: entity_id.clone(),
            }));
        }

        if !self.mutator.instance_exists(target, instance, entity_id, &attr_iri).await? {
            return Err(TemporalError::NotFound(TemporalNotFoundReason::AttributeInstance {
                instance: instance.rendered(InstanceFamily::Attribute).to_string(),
            }));
        }

        let expanded = self.engine.expand(payload, context).await?;
        let instance_value = expanded.as_array().and_then(|arr| arr.first()).cloned().unwrap_or(expanded); // clone: extracting first element from array requires owned value; original consumed by unwrap_or
        validate_instance_unit_code_admission(attr_iri.as_str(), &instance_value)?;

        self.mutator.update_instance(target, instance, entity_id, &attr_iri, &instance_value).await
    }

    /// Delete a specific attribute instance by its instance ID.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::NotFound`] when the entity, the attribute or
    /// the named instance does not exist; [`TemporalError::Validation`] when
    /// the attribute name does not expand to a valid one; and
    /// [`TemporalError::Persistence`] when the delivery barrier cannot drain
    /// the entity's pending history or the deletion cannot be applied.
    pub async fn delete_attr_instance(
        &self,
        target: &ContextTarget,
        entity_id: &Uri,
        attr_id: &AttributeName,
        instance: &InstanceIdentity,
        context: &ContextSpec,
    ) -> Result<(), TemporalError> {
        let timer = temporal::start(TemporalOperation::DeleteAttributeInstance);
        let result = self.delete_attr_instance_inner(target, entity_id, attr_id, instance, context).await;
        timer.finish_result(&result);
        result
    }

    /// The measured body of [`TemporalService::delete_attr_instance`].
    async fn delete_attr_instance_inner(
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
            return Err(TemporalError::NotFound(TemporalNotFoundReason::Entity { id: entity_id.clone() }));
        }

        if !self.mutator.attribute_exists(target, entity_id, &attr_iri).await? {
            return Err(TemporalError::NotFound(TemporalNotFoundReason::AttributeOnEntity {
                attribute: attr_iri.clone(),
                entity: entity_id.clone(),
            }));
        }

        if !self.mutator.instance_exists(target, instance, entity_id, &attr_iri).await? {
            return Err(TemporalError::NotFound(TemporalNotFoundReason::AttributeInstance {
                instance: instance.rendered(InstanceFamily::Attribute).to_string(),
            }));
        }

        self.mutator.delete_instance(target, instance, entity_id, &attr_iri).await?;
        Ok(())
    }
}
