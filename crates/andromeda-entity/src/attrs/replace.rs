//! Replacing one attribute wholesale (clause 5.6.19).

use crate::{
    EntityService,
    admitted_write::AdmittedAttribute,
    attrs::attribute_write::{MutatedEntity, payload_write},
    entity_validation_reason::EntityValidationReason,
    error::EntityError,
    expanded_member::{unwrap_expanded_ref, written_entity_types},
    notification_plan::modification,
    write_kind::ReplaceAttribute,
};
use andromeda_context_commit::history_record::mutation_history;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::expanded::{
    attribute_system_time::stamp_attribute_instants,
    mutation::{attribute::replace_attr, error::ExpandedEntityError},
    written_observation::AddressedInstances,
};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;

impl EntityService {
    /// Replace one attribute of an entity wholesale with the admitted
    /// Attribute Fragment (clause 5.6.19).
    ///
    /// Clauses 5.6.4 and 5.6.19 take a bare Attribute Fragment, which the
    /// admission normalized and expanded under the name the request URI
    /// carries, because a term's own definition decides what the members below
    /// it mean -- see `andromeda_jsonld::attribute_fragment`.
    ///
    /// The replacement is computed from a payload read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Validation`] when the attribute name is a JSON-LD
    /// keyword or names `scope`, or when the fragment cannot replace the stored
    /// instance, [`EntityError::JsonLd`] when the attribute name cannot be
    /// expanded against `context`,
    /// [`EntityError::NotFound`] when no entity carries `id`,
    /// [`EntityError::MemberNotFound`] when the entity carries neither the
    /// attribute nor the selected instance, [`EntityError::Contention`] when
    /// concurrent writers superseded the computed write on every attempt, and
    /// [`EntityError::Persistence`] when the store could not apply it.
    pub async fn replace_attr(
        &self,
        target: &ContextTarget,
        id: &Identifier,
        dataset_id: Option<&Identifier>,
        admitted: AdmittedAttribute<'_, ReplaceAttribute>,
        context: &ContextSpec,
    ) -> Result<(), EntityError> {
        let entity_uri = id.clone(); // clone: notification and computed write own the identifier
        let attr_name = admitted.attribute();

        if attr_name.as_str().starts_with('@') {
            return Err(EntityError::Validation(EntityValidationReason::ReservedAttributeName {
                name: attr_name.clone(),
            }));
        }

        let prepared_context = self.engine.prepare_term_context(context);
        let attr_iri = self.engine.expand_term(attr_name.as_str(), &prepared_context).await?;

        if attr_iri.as_str() == expanded_properties::SCOPE.as_str() {
            return Err(EntityError::Validation(EntityValidationReason::ScopeNotReplaceable));
        }

        let expanded = Arc::new(admitted.into_expanded());

        let MutatedEntity { events } = self
            .commit_computed_write(target, &entity_uri, |previous| {
                let now = Utc::now();
                let mut existing = Value::clone(previous); // clone: the replacement mutates its own copy of the stored payload
                replace_attr(&mut existing, &attr_iri, dataset_id, unwrap_expanded_ref(&expanded))?;

                // Clause 4.8: a touched instance takes `now` for modifiedAt and keeps

                // its createdAt across the read-modify-write.

                stamp_attribute_instants(&mut existing, previous, now).map_err(ExpandedEntityError::from)?;

                let merged = Arc::new(existing);
                let types = written_entity_types(&merged)?;

                let change = modification(previous, &merged)?;
                let plan = self.notifier.plan(target, &entity_uri, &types, change, Some(&merged), Some(previous));

                let addressed = AddressedInstances::AttributeFragment {
                    attribute: &attr_iri,
                    dataset_id,
                    fragment: unwrap_expanded_ref(&expanded),
                };
                let history = mutation_history(addressed, previous, &merged)?;
                let write = payload_write(&entity_uri, &merged, history, plan.matches)?;
                Ok((write, MutatedEntity { events: plan.events }))
            })
            .await?;

        for event in events {
            self.notifier.publish(target, event);
        }

        Ok(())
    }
}
