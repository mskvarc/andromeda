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
    temporal_validation_reason::TemporalValidationReason,
    unit_code_admission::validate_instance_unit_code_admission,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, instance_identity::InstanceIdentity};
use andromeda_ngsi_ld_model::expanded::geo_property_admission::validate_instance_geo_property;
use andromeda_telemetry::temporal::{self, TemporalOperation};
use serde_json::Value;

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
    /// defines without one or carries a geometry clause 4.7 gives no reading
    /// of; [`TemporalError::JsonLd`] when the Fragment cannot be expanded
    /// against `context`; and [`TemporalError::Persistence`] when the
    /// delivery barrier cannot drain the entity's pending history or the
    /// update cannot be applied.
    pub async fn modify_attr_instance(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        attr_id: &AttributeName,
        instance: &InstanceIdentity,
        fragment: Value,
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
        fragment: Value,
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
        // The budget is counted before the expansion multiplies the Fragment.
        self.limits.payload_budget.admit(&fragment)?;
        let expanded = self.engine.expand(fragment, context).await?;
        let instance_value = fragment_instance(expanded, &attr_iri)?;
        validate_instance_unit_code_admission(attr_iri.as_str(), &instance_value)?;
        validate_instance_geo_property(attr_iri.as_str(), &instance_value).map_err(TemporalValidationReason::from)?;

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
fn fragment_instance(expanded: Value, target: &AttributeName) -> Result<Value, TemporalValidationReason> {
    let without_target = || TemporalValidationReason::FragmentWithoutTargetAttribute {
        // clone: the error owns the attribute it names
        target: target.clone(),
    };
    let Value::Array(nodes) = expanded else {
        return Err(without_target());
    };
    let [Value::Object(mut node)] = <[Value; 1]>::try_from(nodes).map_err(|_| without_target())? else {
        return Err(without_target());
    };
    let Some(Value::Array(instances)) = node.remove(target.as_str()) else {
        return Err(without_target());
    };
    let [instance] = <[Value; 1]>::try_from(instances).map_err(|_| without_target())?;
    Ok(instance)
}

#[cfg(test)]
mod tests {
    use crate::{attribute_instance_mutation::fragment_instance, temporal_validation_reason::TemporalValidationReason};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    const SPEED: &str = "https://example.com/speed";

    fn speed() -> AttributeName {
        SPEED.parse().expect("valid attribute name")
    }

    fn expanded_instance() -> serde_json::Value {
        json!({"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 129}]})
    }

    #[test]
    fn the_one_instance_of_the_target_attribute_is_taken() {
        let expanded = json!([{SPEED: [expanded_instance()]}]);
        assert_eq!(fragment_instance(expanded, &speed()).expect("the target instance"), expanded_instance());
    }

    /// A Fragment naming another Attribute than the request targets carries no
    /// instance of the target.
    #[test]
    fn a_fragment_naming_another_attribute_is_refused() {
        let expanded = json!([{"https://example.com/fuel": [expanded_instance()]}]);
        assert!(matches!(
            fragment_instance(expanded, &speed()),
            Err(TemporalValidationReason::FragmentWithoutTargetAttribute { .. })
        ));
    }

    /// Two compact terms expanding to the target would merge into two instances.
    #[test]
    fn two_instances_of_the_target_are_refused() {
        let expanded = json!([{SPEED: [expanded_instance(), expanded_instance()]}]);
        assert!(matches!(
            fragment_instance(expanded, &speed()),
            Err(TemporalValidationReason::FragmentWithoutTargetAttribute { .. })
        ));
    }

    /// A Fragment whose member JSON-LD expansion drops expands to no node.
    #[test]
    fn a_fragment_expanding_to_nothing_is_refused() {
        assert!(matches!(
            fragment_instance(json!([]), &speed()),
            Err(TemporalValidationReason::FragmentWithoutTargetAttribute { .. })
        ));
    }
}
