//! Updating the attributes an entity already has (clause 5.6.2).

use crate::{
    EntityService,
    admitted_write::AdmittedEntity,
    attrs::attribute_write::payload_write,
    error::EntityError,
    expanded_member::{unwrap_expanded_ref, written_attribute_names, written_entity_types},
    notification_plan::attribute_change_groups,
    write_kind::UpdateAttributes,
};
use andromeda_context_commit::{change_event::ChangeEvent, history_record::mutation_history};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::expanded::{
    attribute_system_time::stamp_attribute_instants,
    mutation::{entity::update_entity_attrs, error::ExpandedEntityError, selection::OverwritePolicy},
    written_observation::AddressedInstances,
};
use chrono::Utc;
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
    /// The update is computed from a payload read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Validation`] when an attribute instance cannot be
    /// merged into the stored one, [`EntityError::NotFound`] when no entity
    /// carries `id`,
    /// [`EntityError::Contention`] when concurrent writers superseded the
    /// computed write on every attempt, and [`EntityError::Persistence`] when
    /// the store could not apply it.
    pub async fn update_attrs(
        &self,
        target: &ContextTarget,
        id: &Uri,
        admitted: AdmittedEntity<UpdateAttributes>,
    ) -> Result<(Vec<AttributeName>, Vec<AttributeName>), EntityError> {
        let entity_uri = id.clone(); // clone: notification and computed write own the identifier
        let expanded = Arc::new(admitted.into_expanded());
        // Clause 5.6.2.5 reports the attributes this request asked for, so the
        // set is read from the fragment before the write merges it away.
        let requested = written_attribute_names(unwrap_expanded_ref(&expanded))?;
        let (events, not_updated): (Vec<ChangeEvent>, Vec<AttributeName>) = self
            .commit_computed_write(target, &entity_uri, |previous| {
                let now = Utc::now();
                let mut existing = previous.clone(); // clone: the update mutates its own copy of the stored payload
                let not_updated = update_entity_attrs(&mut existing, unwrap_expanded_ref(&expanded))?;

                // Clause 4.8: a touched instance takes `now` for modifiedAt and keeps

                // its createdAt across the read-modify-write.

                stamp_attribute_instants(&mut existing, previous, now).map_err(ExpandedEntityError::from)?;

                let merged = Arc::new(existing);
                let types = written_entity_types(&merged)?;

                // The durable matches and the in-process events are planned
                // together from one partition, so the two channels always agree
                // on the per-attribute triggers (clause 5.2.12).
                let groups = attribute_change_groups(previous, &merged)?;
                let plan = self.plan_notifications(target, &entity_uri, &types, &groups, Some(&merged), Some(previous));

                let addressed = AddressedInstances::EntityFragment {
                    fragment: &expanded,
                    overwrite: OverwritePolicy::Overwrite,
                };
                let history = mutation_history(addressed, previous, &merged)?;
                let write = payload_write(&entity_uri, &merged, history, plan.matches)?;
                Ok((write, (plan.events, not_updated)))
            })
            .await?;

        for event in events {
            self.emit_change(target, event);
        }

        Ok((requested, not_updated))
    }
}
