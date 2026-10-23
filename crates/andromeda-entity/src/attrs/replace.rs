//! Replacing one attribute wholesale (clause 5.6.19).

use crate::{
    EntityService,
    admitted_write::AdmittedAttribute,
    entity_validation_reason::EntityValidationReason,
    error::EntityError,
    write_kind::ReplaceAttribute,
    write_target::WriteTarget,
};
use andromeda_context_commit::history_record::mutation_history;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use std::sync::Arc;

impl EntityService {
    /// Replace one attribute of an entity wholesale with the admitted
    /// Attribute Fragment (clause 5.6.19).
    ///
    /// Clauses 5.6.4 and 5.6.19 take a bare Attribute Fragment, which the
    /// admission normalized and expanded under the name the request URI
    /// carries, because a term's own definition decides what the members below
    /// it mean.
    ///
    /// The replacement is computed from a state read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Validation`] when the attribute name is a JSON-LD
    /// keyword or names `scope`, or when the fragment cannot replace the stored
    /// instance, [`EntityError::NotFound`] when no entity carries the
    /// identifier of `entity` or the type selector of `entity` excludes it
    /// (clause 5.6.19.4), [`EntityError::MemberNotFound`] when the entity carries neither the
    /// attribute nor the selected instance, [`EntityError::Contention`] when
    /// concurrent writers superseded the computed write on every attempt, and
    /// [`EntityError::Persistence`] when the store could not apply it.
    pub async fn replace_attr(
        &self,
        target: &ContextTarget,
        entity: WriteTarget<'_>,
        admitted: AdmittedAttribute<'_, ReplaceAttribute>,
    ) -> Result<(), EntityError> {
        let id = entity.id;
        let attr_name = admitted.attribute();
        if attr_name.as_str().starts_with('@') {
            return Err(EntityError::Validation(EntityValidationReason::ReservedAttributeName {
                name: attr_name.clone(), // clone: the error owns the name it refuses
            }));
        }
        if admitted.expanded_name().as_str() == expanded_properties::SCOPE.as_str() {
            return Err(EntityError::Validation(EntityValidationReason::ScopeNotReplaceable));
        }
        let (attribute, replacement) = admitted.into_parts();

        let events = self
            .commit_computed_write(target, entity, |previous, now| {
                let mut replaced = EntityState::clone(previous); // clone: shares every attribute the replacement leaves alone
                replaced.replace_attribute(&attribute, &replacement)?;
                // Clause 4.8: a touched instance takes `now` for modifiedAt and keeps
                // its createdAt across the read-modify-write.
                replaced.settle(Some(previous), now);
                let replaced = Arc::new(replaced);
                let history = mutation_history(previous, &replaced);
                let (write, events) = self.write_if_modified(target, id, previous, replaced, history)?;
                Ok((write, events))
            })
            .await?;

        for event in events {
            self.notifier.publish(target, event);
        }

        Ok(())
    }
}
