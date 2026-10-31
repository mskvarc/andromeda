//! Temporal attribute-instance mutation policy.
//!
//! The instance-level operations of clauses 5.6.14 and 5.6.15: replacing one
//! recorded attribute instance and deleting one recorded attribute instance,
//! each addressed by its `instanceId`. Both hang off [`TemporalService`] and
//! share the attribute-name resolution and the mutation barrier with the
//! entity- and attribute-level operations in [`crate::write`].

use crate::{
    error::TemporalError,
    observed_instance::ObservedInstance,
    service::TemporalService,
    temporal_not_found_reason::TemporalNotFoundReason,
    temporal_validation_reason::TemporalValidationReason,
    unit_code_admission::admit_instance_unit_code,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::document_census::admit_instance_document;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::{
    admission::{attribute_nesting_admission::admit_instance_nesting, null_sentinel_admission::NullSentinelAdmission},
    document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument},
};
use andromeda_ngsi_ld_expansion::{entity_expansion::EntityExpansion, normalization::Normalization};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, instance_identity::InstanceIdentity};
use andromeda_ngsi_ld_model::temporal::written_temporal_entity::WrittenTemporalEntity;
use andromeda_telemetry::temporal::{self, TemporalOperation};

impl TemporalService {
    /// Replace an existing attribute instance with the one instance the clause
    /// 5.6.14.3 `EntityTemporal` `fragment` carries.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::NotFound`] when the entity, the attribute or
    /// the named instance does not exist; [`TemporalError::Validation`] when
    /// the attribute name does not expand to a valid one, the Fragment does
    /// not carry exactly one instance of the target attribute, or the
    /// replacement puts a `unitCode` on an attribute family clause 4.5.2
    /// defines without one, carries a geometry clause 4.7 gives no reading
    /// of, or holds the NGSI-LD Null under a key of a Property value (clause
    /// 5.5.4); [`TemporalError::JsonLd`] when the Fragment cannot be expanded
    /// against `context`; [`TemporalError::HistoryUnsettled`] when another
    /// delivery owner still holds the entity's queued history once the
    /// mutation barrier's wait runs out; the failure of a queued intent that
    /// does not apply; and [`TemporalError::Persistence`] when the update
    /// cannot be applied.
    pub async fn modify_attr_instance(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        attr_id: &AttributeName,
        instance: &InstanceIdentity,
        fragment: WrittenTemporalEntity,
        context: &ContextSpec,
    ) -> Result<(), TemporalError> {
        let timer = temporal::start(TemporalOperation::ModifyAttributeInstance);
        let result = self.modify_attr_instance_inner(target, entity_id, attr_id, instance, fragment, context).await;
        timer.finish_result(&result);
        result
    }

    /// The measured body of [`TemporalService::modify_attr_instance`].
    async fn modify_attr_instance_inner(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        attr_id: &AttributeName,
        instance: &InstanceIdentity,
        fragment: WrittenTemporalEntity,
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
                instance: instance.clone(), // clone: the error owns the instance identity it names.
            }));
        }

        // Clause 5.6.14.4 raises ResourceNotFound for an unknown Entity,
        // Attribute or instance before the Fragment is read, so a Fragment
        // naming another Attribute is refused only once the target is known.
        let expansion = EntityExpansion::prepare(&self.engine, context)
            .await?
            .bounded_by(self.limits.payload_budget.json_values);
        let (patch, _reified_scope) = fragment.into_fragment();
        let document = expansion.entity_patch(&patch, Normalization::Entity).await?;
        let replacement = fragment_instance(document, &attr_iri)?;
        admit_instance_document(self.limits.payload_budget, &attr_iri, &replacement)?;
        admit_instance_nesting(self.limits.payload_budget.attribute_nesting, &attr_iri, &replacement)?;
        admit_instance_unit_code(&attr_iri, &replacement)?;
        replacement.admit_datatypes(&attr_iri)?;
        // Clause 5.5.4: the temporal evolution is excepted from the second
        // NGSI-LD Null prohibition, never from the third.
        replacement.admit_null_placements(&attr_iri, NullSentinelAdmission::TemporalEvolution)?;
        // Clause 5.6.14 replaces the instance with the one the Fragment
        // carries, so a replacement recording no value replaces it with none.
        let observation = ObservedInstance::read(replacement)?.ok_or_else(|| TemporalValidationReason::ReplacementRecordsNoValue {
            target: attr_iri.clone(), // clone: the error owns the attribute it names
        })?;

        self.mutator.update_instance(target, instance, entity_id, &attr_iri, &observation).await
    }

    /// Delete a specific attribute instance by its instance ID.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::NotFound`] when the entity, the attribute or
    /// the named instance does not exist; [`TemporalError::Validation`] when
    /// the attribute name does not expand to a valid one;
    /// [`TemporalError::HistoryUnsettled`] when another delivery owner still
    /// holds the entity's queued history once the mutation barrier's wait runs
    /// out; the failure of a queued intent that does not apply; and
    /// [`TemporalError::Persistence`] when the deletion cannot be applied.
    pub async fn delete_attr_instance(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
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
        entity_id: &Identifier,
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
                instance: instance.clone(), // clone: the error owns the instance identity it names.
            }));
        }

        self.mutator.delete_instance(target, instance, entity_id, &attr_iri).await?;
        Ok(())
    }
}

/// The one instance of `target` an expanded clause 5.6.14.3 `EntityTemporal`
/// Fragment carries.
///
/// The Fragment's shape -- one Attribute, an Array of exactly one instance --
/// is admitted at the binding edge before anything is forwarded; what only
/// expansion can decide is whether that Attribute is the one the request
/// targets.
///
/// # Errors
///
/// Returns [`TemporalValidationReason::FragmentWithoutTargetAttribute`] when
/// the Fragment does not expand to exactly one instance of `target`.
fn fragment_instance(document: EntityDocument, target: &AttributeName) -> Result<InstanceDocument, TemporalValidationReason> {
    let without_target = || TemporalValidationReason::FragmentWithoutTargetAttribute {
        // clone: the error owns the attribute it names
        target: target.clone(),
    };
    let mut attributes = document.into_parts().attributes;
    let Some(AttributeMember::Instances(instances)) = attributes.shift_remove(target) else {
        return Err(without_target());
    };
    let [instance] = <[InstanceDocument; 1]>::try_from(instances).map_err(|_| without_target())?;
    Ok(instance)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{attribute_instance_mutation::fragment_instance, temporal_validation_reason::TemporalValidationReason};
    use andromeda_ngsi_ld_entity::document::EntityDocument;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use serde_json::{Value, json};

    const SPEED: &str = "https://example.com/speed";

    fn speed() -> AttributeName {
        SPEED.parse().expect("valid attribute name")
    }

    fn expanded_instance() -> Value {
        json!({"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 129}]})
    }

    fn document(expanded: &Value) -> EntityDocument {
        EntityDocument::from_slice(&serde_json::to_vec(expanded).expect("JSON")).expect("a written fragment decodes")
    }

    #[test]
    fn the_one_instance_of_the_target_attribute_is_taken() {
        let instance = fragment_instance(document(&json!([{SPEED: [expanded_instance()]}])), &speed()).expect("the target instance");
        let written: Value = serde_json::to_value(&instance).expect("the instance writes");
        pretty_assertions::assert_eq!(written, expanded_instance());
    }

    /// A Fragment naming another Attribute than the request targets carries no
    /// instance of the target.
    #[test]
    fn a_fragment_naming_another_attribute_is_refused() {
        assert!(matches!(
            fragment_instance(document(&json!([{"https://example.com/fuel": [expanded_instance()]}])), &speed()),
            Err(TemporalValidationReason::FragmentWithoutTargetAttribute { .. })
        ));
    }

    /// Two compact terms expanding to the target would merge into two instances.
    #[test]
    fn two_instances_of_the_target_are_refused() {
        assert!(matches!(
            fragment_instance(document(&json!([{SPEED: [expanded_instance(), expanded_instance()]}])), &speed()),
            Err(TemporalValidationReason::FragmentWithoutTargetAttribute { .. })
        ));
    }

    /// A Fragment whose member JSON-LD expansion drops expands to no attribute.
    #[test]
    fn a_fragment_expanding_to_nothing_is_refused() {
        assert!(matches!(
            fragment_instance(document(&json!([{}])), &speed()),
            Err(TemporalValidationReason::FragmentWithoutTargetAttribute { .. })
        ));
    }
}
