//! Partially updating one attribute instance (clause 5.6.4).

use crate::{
    EntityService,
    admitted_write::AdmittedAttribute,
    entity_validation_reason::EntityValidationReason,
    error::EntityError,
    modification_write::ModificationWrite,
    write_kind::PartialUpdateAttribute,
};
use andromeda_context_commit::history_record::mutation_history;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::{entity_state::EntityState, history::written_observation::AddressedInstances, system_instant::SystemInstant};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use chrono::Utc;
use std::sync::Arc;

impl EntityService {
    /// Apply the admitted Attribute Fragment as a partial update of one
    /// attribute instance (clause 5.6.4).
    ///
    /// Clauses 5.6.4 and 5.6.19 take a bare Attribute Fragment, which the
    /// admission normalized and expanded under the name the request URI
    /// carries, because a term's own definition decides what the members below
    /// it mean. Clause 5.5.8 makes its NGSI-LD Nulls deletions, with one
    /// exception it states outright: "A datasetId cannot be deleted by setting
    /// it to the value urn:ngsi-ld:null".
    ///
    /// The update is computed from a state read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Validation`] when the attribute names `scope`
    /// (clause 5.6.4.4) or the fragment cannot be merged into the stored
    /// instance, [`EntityError::NotFound`] when no entity carries `id`,
    /// [`EntityError::MemberNotFound`] when the entity carries neither the
    /// attribute nor the selected instance, [`EntityError::Contention`] when
    /// concurrent writers superseded the computed write on every attempt, and
    /// [`EntityError::Persistence`] when the store could not apply it.
    pub async fn partial_update_attr(
        &self,
        target: &ContextTarget,
        id: &Identifier,
        dataset_id: Option<&Identifier>,
        admitted: AdmittedAttribute<'_, PartialUpdateAttribute>,
    ) -> Result<(), EntityError> {
        // Clause 5.6.4.4: `scope` is not an attribute and cannot be a partial
        // update target, exactly as Replace Attribute rejects it.
        if admitted.expanded_name().as_str() == expanded_properties::SCOPE.as_str() {
            return Err(EntityError::Validation(EntityValidationReason::ScopeNotPartiallyUpdatable));
        }
        let (attribute, fragment) = admitted.into_parts();

        let events = self
            .commit_computed_write(target, id, |previous| {
                let mut updated = EntityState::clone(previous); // clone: shares every attribute the update leaves alone
                updated.partial_update_attribute(&attribute, dataset_id, &fragment)?;
                // Clause 4.8: a touched instance takes `now` for modifiedAt and keeps
                // its createdAt across the read-modify-write.
                updated.stamp_attribute_instants(Some(previous), SystemInstant::new(Utc::now()));
                let updated = Arc::new(updated);
                let addressed = AddressedInstances::AttributeFragment {
                    attribute: &attribute,
                    dataset_id,
                    fragment: &fragment,
                };
                let history = mutation_history(addressed, previous, &updated);
                let ModificationWrite { write, events } = self.modification_write(target, id, previous, updated, history)?;
                Ok((write, events))
            })
            .await?;

        for event in events {
            self.notifier.publish(target, event);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        admission_fixture::admitted_attribute,
        error::EntityError,
        notification_recording_harness::{entity_uri, live_target, recording_service},
        write_kind::PartialUpdateAttribute,
    };
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use serde_json::json;

    #[tokio::test]
    async fn partial_update_attr_rejects_the_scope_target() {
        // ETSI GS CIM 009 v1.9.1 clause 5.6.4.4: `scope` is not a partial-update
        // target and must be refused as BadRequestData, as Replace Attribute is.
        let (service, _recorded) = recording_service().await;
        let scope: AttributeName = "scope".parse().expect("scope is a valid attribute name");
        let admitted =
            admitted_attribute::<PartialUpdateAttribute>(&service, &scope, json!({"type": "Property", "value": "/Madrid"}), &ContextSpec::core_only()).await;
        let error = service
            .partial_update_attr(&live_target(), &entity_uri(), None, admitted)
            .await
            .expect_err("scope cannot be partially updated");
        assert!(matches!(error, EntityError::Validation(_)), "{error}");
    }
}
