//! Delete Entity (clause 5.6.6).

use crate::{
    EntityService,
    error::EntityError,
    expanded_member::{stored_attribute_names, stored_entity_types},
    notification_fanout::{ChangedEntity, MutationSnapshots},
    sys_attrs::snapshot_with_sys_attrs,
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    change_event::ChangeEvent,
    entity_mutation::EntityMutation,
    history_record::deletion_history,
    precondition::Precondition,
    write_outcome::single,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    expanded::attribute_change::classify_attribute_changes,
    notification::{ChangeOrigin, NotificationTrigger},
};
use andromeda_telemetry::entity::{self, EntityOperation};
use chrono::Utc;
use serde_json::Value;
use std::{slice, sync::Arc};

impl EntityService {
    /// Delete an entity by its URI (NGSI-LD 5.6.6).
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::NotFound`] when no entity carries `id`, and
    /// [`EntityError::Persistence`] when the store could not read the entity or
    /// commit the deletion.
    pub async fn delete(&self, target: &ContextTarget, id: &Uri) -> Result<(), EntityError> {
        let timer = entity::start(EntityOperation::Delete);
        let result: Result<(), EntityError> = async {
            let entity_uri = id.clone(); // clone: delete events and persistence work own the identifier
            let id = id.as_str();

            let entity_snapshot = self.reader.get(target, &entity_uri).await?.ok_or_else(|| EntityError::not_found(&entity_uri))?;
            let entity_types = stored_entity_types(&entity_snapshot)?;

            // Captured before the delete so the notification can carry them: once
            // the row is gone they are unreadable.
            let sys_attrs_snapshot = match self.reader.get_sys_attrs(target, &entity_uri).await {
                Ok(attrs) => attrs,
                Err(e) => {
                    tracing::warn!("Failed to fetch sys_attrs before delete for {}: {}", id, e);
                    None
                }
            };

            let attr_keys = stored_attribute_names(&entity_snapshot)?;
            let attribute_changes = classify_attribute_changes(&entity_snapshot, &Value::Null).map_err(EntityError::internal)?;

            // The durable match stores the entity snapshot verbatim, and by delivery
            // time the deleted row is gone, so its clause-4.8 system attributes are
            // captured into the snapshot now rather than re-read then (the durable
            // path re-derives only `deletedAt`, from the event instant).
            let match_snapshot = sys_attrs_snapshot
                .as_ref()
                .map_or_else(|| entity_snapshot.clone(), |sys_attrs| snapshot_with_sys_attrs(&entity_snapshot, sys_attrs));

            let deleted = ChangedEntity {
                target,
                id: &entity_uri,
                types: &entity_types,
            };
            let snapshots = MutationSnapshots {
                current: Some(&match_snapshot),
                previous: Some(&entity_snapshot),
            };
            let mut subscription_matches = Vec::new();
            for change in &attribute_changes {
                subscription_matches.extend(self.build_subscription_matches_for_instance(
                    deleted,
                    NotificationTrigger::AttributeDeleted,
                    slice::from_ref(&change.key.attr),
                    change.key.dataset_id.as_ref(),
                    snapshots,
                ));
            }
            subscription_matches.extend(self.build_subscription_matches(deleted, NotificationTrigger::EntityDeleted, &attr_keys, snapshots));

            let write = AtomicWrite::builder()
                .entity_id(Arc::new(entity_uri.clone())) // clone: the delete write owns its validated identifier.
                .entity(EntityMutation::Delete)
                // Clause 5.6.6 removes current state and leaves the Temporal
                // Evolution standing, so every instance the entity still held is
                // recorded as ended rather than being deleted with it.
                .history(deletion_history(&entity_snapshot)?)
                .subscription_matches(subscription_matches)
                .precondition(Precondition::MustExist)
                .build();

            if single(self.writes.commit(target, vec![write]).await?)?.is_not_found() {
                return Err(EntityError::not_found(&entity_uri));
            }

            // One event per deleted attribute instance so dataset-scoped
            // `attributeDeleted` subscriptions receive only the instance removed.
            for change in attribute_changes {
                self.emit_change(
                    target,
                    ChangeEvent::builder()
                        .entity_id(entity_uri.clone()) // clone: value also needed by subsequent EntityDeleted emit
                        .entity_types(entity_types.clone()) // clone: value also needed by subsequent EntityDeleted emit
                        .trigger_reason(NotificationTrigger::AttributeDeleted)
                        .occurred_at(Utc::now())
                        .origin(ChangeOrigin::Local)
                        .changed_attributes(vec![change.key.attr])
                        .changed_dataset_id(change.key.dataset_id)
                        .entity_snapshot(Some(entity_snapshot.clone())) // clone: value also needed by subsequent EntityDeleted emit
                        .previous_attributes(Some(entity_snapshot.clone())) // clone: value also needed by subsequent EntityDeleted emit
                        .sys_attrs_snapshot(sys_attrs_snapshot.clone()) // clone: value also needed by subsequent EntityDeleted emit
                        .build(),
                );
            }

            self.emit_change(
                target,
                ChangeEvent::builder()
                    .entity_id(entity_uri)
                    .entity_types(entity_types)
                    .trigger_reason(NotificationTrigger::EntityDeleted)
                    .occurred_at(Utc::now())
                    .origin(ChangeOrigin::Local)
                    .changed_attributes(attr_keys)
                    .entity_snapshot(Some(entity_snapshot.clone())) // clone: separate ownership for snapshot and previous fields
                    .previous_attributes(Some(entity_snapshot))
                    .sys_attrs_snapshot(sys_attrs_snapshot)
                    .build(),
            );

            Ok(())
        }
        .await;
        timer.finish_result(&result);
        result
    }
}
