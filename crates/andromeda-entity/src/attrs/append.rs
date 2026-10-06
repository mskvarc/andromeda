//! Appending attributes to an entity (clause 5.6.3).

use crate::{
    EntityService,
    admitted_write::AdmittedEntity,
    error::EntityError,
    write_kind::AppendAttributes,
    write_target::WriteTarget,
    written_members::{member_attribute_names, written_member_names},
};
use andromeda_context_commit::history_record::mutation_history;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::{
    entity_state::EntityState,
    history::written_observation::AddressedInstances,
    operation::selection::OverwritePolicy,
    system_instant::SystemInstant,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use chrono::Utc;
use std::sync::Arc;

impl EntityService {
    /// Append the attributes of the admitted Entity Fragment to an existing
    /// entity (clause 5.6.3).
    ///
    /// Clause 5.6.3.4 folds the fragment's `scope` into the target's; it is not
    /// one of the partial update or merge fragments clause 5.5.4 excepts from
    /// its first-level NGSI-LD Null prohibition, so the fragment was admitted
    /// without a deletion sentinel.
    ///
    /// Returns `(requested, not_appended)`, both by expanded name:
    /// - every member the fragment wrote
    /// - the members that were NOT appended, because `noOverwrite` was set
    ///   and the instance already existed
    ///
    /// The append is computed from a state read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Validation`] when an attribute instance cannot be
    /// appended to the stored ones, [`EntityError::NotFound`] when no entity
    /// carries the identifier of `entity` or the type selector of `entity`
    /// excludes it (clause 5.6.3.4), [`EntityError::Contention`] when concurrent writers
    /// superseded the computed write on every attempt, and
    /// [`EntityError::Persistence`] when the store could not apply it.
    pub async fn append_attrs(
        &self,
        target: &ContextTarget,
        entity: WriteTarget<'_>,
        admitted: AdmittedEntity<AppendAttributes>,
        overwrite: OverwritePolicy,
    ) -> Result<(Vec<AttributeName>, Vec<AttributeName>), EntityError> {
        let id = entity.id;
        let fragment = admitted.into_document();

        // Clause 5.6.3.5 reports "the list of Attributes actually appended", so
        // the reported set starts from the fragment's own members. Reading it
        // from the merged entity instead would report every attribute the entity
        // already held as if this request had appended it.
        let requested = written_member_names(&fragment)?;
        let (not_appended, events) = self
            .commit_computed_write(target, entity, |previous| {
                let mut appended = EntityState::clone(previous); // clone: shares every attribute the append leaves alone
                let not_appended = member_attribute_names(appended.append_attributes(&fragment, overwrite)?)?;
                // Clause 4.8: a touched instance takes `now` for modifiedAt and keeps
                // its createdAt across the read-modify-write.
                appended.stamp_attribute_instants(Some(previous), SystemInstant::new(Utc::now()));
                let appended = Arc::new(appended);
                let addressed = AddressedInstances::EntityFragment {
                    fragment: &fragment,
                    overwrite,
                };
                let history = mutation_history(addressed, previous, &appended);
                let (write, events) = self.fragment_write(target, id, previous, appended, history)?;
                Ok((write, (not_appended, events)))
            })
            .await?;

        for event in events {
            self.notifier.publish(target, event);
        }

        Ok((requested, not_appended))
    }
}
