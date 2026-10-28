//! Updating the attributes an entity already has (clause 5.6.2).

use crate::{
    EntityService,
    admitted_write::AdmittedEntity,
    error::EntityError,
    write_kind::UpdateAttributes,
    write_target::WriteTarget,
    written_members::{member_attribute_names, written_member_names},
};
use andromeda_context_commit::{change_event::ChangeEvent, history_record::mutation_history};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use std::sync::Arc;

impl EntityService {
    /// Update the attributes an entity already carries (clause 5.6.2) with
    /// the admitted Entity Fragment.
    ///
    /// Clause 5.5.8 EXAMPLE 3 deletes an Attribute by setting its value to the
    /// NGSI-LD Null, so an Update Attributes fragment was admitted carrying the
    /// sentinel at the first level and in the value position; clause 5.5.4
    /// excepts merge operations alone from nesting it inside a value object.
    /// Clause 5.5.8 reads the same sentinel on `scope` as the deleted scope of
    /// clause 4.18.
    ///
    /// Returns the attribute IRIs the fragment asked to update, and those that
    /// were NOT updated because the entity did not already carry them. Clause
    /// 5.6.2.5 makes both halves output data: a status code saying whether all
    /// or only some attributes were updated, and "the list of Attributes
    /// (Properties or Relationships) actually updated". The requested set is
    /// the fragment's own attributes, not the merged entity's, so an attribute
    /// the entity already held and this request never mentioned is not reported
    /// as updated.
    ///
    /// The update is computed from a state read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Validation`] when an attribute instance cannot be
    /// merged into the stored one, [`EntityError::NotFound`] when no entity
    /// carries the identifier of `entity` or the type selector of `entity`
    /// excludes it (clause 5.6.2.4), [`EntityError::Contention`] when concurrent writers superseded the
    /// computed write on every attempt, and [`EntityError::Persistence`] when
    /// the store could not apply it.
    pub async fn update_attrs(
        &self,
        target: &ContextTarget,
        entity: WriteTarget<'_>,
        admitted: AdmittedEntity<UpdateAttributes>,
    ) -> Result<(Vec<AttributeName>, Vec<AttributeName>), EntityError> {
        let id = entity.id;
        let fragment = admitted.into_document();
        // Clause 5.6.2.5 reports the attributes this request asked for, so the
        // set is read from the fragment before the write merges it away.
        let requested = written_member_names(&fragment)?;
        let (events, not_updated): (Vec<ChangeEvent>, Vec<AttributeName>) = self
            .commit_computed_write(target, entity, |previous, now| {
                let mut updated = EntityState::clone(previous); // clone: shares every attribute the update leaves alone
                let not_updated = member_attribute_names(updated.update_attributes(&fragment)?)?;
                // Clause 4.8: a touched instance takes `now` for modifiedAt and keeps
                // its createdAt across the read-modify-write.
                updated.settle(Some(previous), now);
                let updated = Arc::new(updated);
                let history = mutation_history(previous, &updated);
                // The durable matches and the in-process events are planned
                // together from one partition, so the two channels always agree
                // on the per-attribute triggers (clause 5.2.12).
                let (write, events) = self.write_if_modified(target, id, previous, updated, history)?;
                Ok((write, (events, not_updated)))
            })
            .await?;

        self.notifier.publish(target, events).await;

        Ok((requested, not_updated))
    }
}
