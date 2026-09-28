//! Replace Entity (clause 5.6.18).

use crate::{
    EntityService,
    admitted_write::AdmittedEntity,
    error::EntityError,
    expanded_member::written_entity_types,
    notification_plan::modification,
    write_kind::ReplaceEntity,
};
use andromeda_context_commit::{atomic_write::AtomicWrite, entity_mutation::EntityMutation, entity_record::EntityRecord, history_record::mutation_history};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::expanded::{
    attribute_system_time::stamp_attribute_instants,
    mutation::{error::ExpandedEntityError, selection::OverwritePolicy},
    written_observation::AddressedInstances,
};
use andromeda_telemetry::entity::{self, EntityOperation};
use chrono::Utc;
use std::sync::Arc;

impl EntityService {
    /// Replace an entire entity with its admitted payload (NGSI-LD 5.6.18).
    ///
    /// The replacement drops every attribute it omits, so each dropped instance
    /// needs a tombstone, and that diff is only meaningful against the payload
    /// the entity actually had when the write landed. The write therefore
    /// states the `modifiedAt` its diff assumed; a writer that got in first
    /// makes the store refuse it, and the entity is read again.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::NotFound`] when no entity carries `id`,
    /// [`EntityError::Validation`] when the payload's members cannot be read,
    /// [`EntityError::Contention`] when concurrent writers superseded the
    /// computed write on every attempt, and [`EntityError::Persistence`] when
    /// the store could not apply it.
    pub async fn replace_entity(&self, target: &ContextTarget, id: &Uri, admitted: AdmittedEntity<ReplaceEntity>) -> Result<(), EntityError> {
        let timer = entity::start(EntityOperation::Replace);
        let result: Result<(), EntityError> = async {
            let entity_uri = id.clone(); // clone: mutation events and computed-write closures own the identifier
            if self.reader.get_snapshot(target, &entity_uri).await?.is_none() {
                return Err(EntityError::not_found(&entity_uri));
            }
            let expanded = admitted.into_expanded();

            let entity_id = Arc::new(entity_uri.clone()); // clone: repeated computed-write attempts share the validated identifier.
            let now = Utc::now();

            // The types the notification carries are the stored ones, read off the
            // payload the replacement actually superseded rather than off an earlier
            // attempt's.
            let events = self
                .commit_computed_write(target, &entity_uri, |previous| {
                    // Clause 4.8: an attribute the replacement keeps (same datasetId)
                    // keeps its createdAt, matching the Entity-level createdAt column
                    // that a replace does not rewrite; every instance takes `now` for
                    // modifiedAt. Stamped per attempt against that attempt's stored
                    // payload, so createdAt survives the replace.
                    let mut replaced = expanded.clone(); // clone: each attempt stamps its own copy against the payload it superseded
                    stamp_attribute_instants(&mut replaced, previous, now).map_err(ExpandedEntityError::from)?;
                    let payload = Arc::new(replaced);
                    let types = written_entity_types(&payload)?;
                    let addressed = AddressedInstances::EntityFragment {
                        fragment: &payload,
                        overwrite: OverwritePolicy::Overwrite,
                    };
                    let history = mutation_history(addressed, previous, &payload)?;
                    let change = modification(previous, &payload)?;
                    let plan = self.notifier.plan(target, &entity_uri, &types, change, Some(&payload), Some(previous));
                    let write = AtomicWrite::builder()
                        .entity_id(Arc::clone(&entity_id)) // clone: Arc ref-count bump; the identifier outlives the attempt
                        .entity(EntityMutation::Update(EntityRecord::from_expanded(Arc::clone(&payload))?)) // clone: Arc ref-count bump
                        .history(history)
                        .subscription_matches(plan.matches)
                        .build();
                    Ok((write, plan.events))
                })
                .await?;

            for event in events {
                self.notifier.publish(target, event);
            }
            Ok(())
        }
        .await;
        timer.finish_result(&result);
        result
    }
}
