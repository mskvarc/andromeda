//! Whole-entity writes: create, merge, replace and delete (clauses 5.6.1,
//! 5.6.17, 5.6.18 and 5.6.6).
//!
//! Attribute-level mutations live in `attrs.rs`; batch forms live in `batch/`.

use crate::{
    EntityService,
    attribute_envelope::validate_attribute_envelopes,
    error::EntityError,
    expanded_member::extract_attribute_keys,
    geo_property_geometry::validate_geo_property_geometries,
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    change_event::ChangeEvent,
    entity_mutation::{EntityMutation, EntityRecord},
    history_record::{HistoryRecord, deletion_history, mutation_history},
    precondition::Precondition,
    write_outcome::single,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    entity::{Entity, EntityPatch},
    expanded::{entity_member::extract_types, mutation::entity::merge_entity},
    notification::{ChangeOrigin, NotificationTrigger},
};
use andromeda_ngsi_ld_representation::normalization::{normalize_entity_attributes, normalize_merge_fragment};
use chrono::Utc;
use serde_json::Value;
use std::{collections::HashSet, slice::from_ref, sync::Arc};

/// What one clause 5.6.17 merge produced, beyond the write itself.
///
/// Read off the attempt that actually landed: the merge runs again whenever a
/// concurrent writer supersedes an attempt, and the notification must describe
/// the change the store accepted rather than one that was computed and lost.
struct MergedEntity {
    /// The entity as it stands after the merge.
    merged: Arc<Value>,
    /// The entity's types after the merge (clause 4.5.1).
    types: Vec<EntityTypeName>,
    /// The attribute names the patch touched.
    changed_attrs: Vec<AttributeName>,
    /// The attribute names the merge removed, each of which needs a tombstone
    /// and an `attributeDeleted` notification.
    deleted_attrs: Vec<AttributeName>,
    /// The payload as it stood before the merge, when a `showChanges`
    /// subscriber needs it (clause 5.2.14).
    previous_attributes: Option<Value>,
}

impl EntityService {
    /// Create a new NGSI-LD entity, returning its URI on success (clause 5.6.1).
    pub async fn create(&self, entity: &Entity, context: &ContextSpec) -> Result<Uri, EntityError> {
        let id = &entity.id;

        let mut entity_json = serde_json::to_value(entity).map_err(|err| EntityError::validation(format!("Invalid entity payload: {}", err)))?;
        normalize_entity_attributes(&mut entity_json);
        let expanded = self.engine.expand(entity_json, context).await?;
        validate_attribute_envelopes(&expanded)?;
        validate_geo_property_geometries(&expanded)?;

        let payload = Arc::new(expanded);
        let types = extract_types(&payload);
        let changed_attrs = extract_attribute_keys(&payload);
        let subscription_matches =
            self.build_subscription_matches(&entity.id, &types, NotificationTrigger::EntityCreated, &changed_attrs, Some(&payload), None);

        let write = AtomicWrite::builder()
            .entity_id(Arc::new(id.clone())) // clone: the write owns a shared validated identifier.
            .entity(EntityMutation::Insert(EntityRecord::from_expanded(Arc::clone(&payload)))) // clone: Arc ref-count bump; the payload is also the change event's
            .history(vec![HistoryRecord::Observations(Arc::clone(&payload))]) // clone: Arc ref-count bump
            .subscription_matches(subscription_matches)
            .precondition(Precondition::MustNotExist)
            .build();

        // A conflicting insert is 409, per NGSI-LD 6.4.3.1. It reaches here as a
        // precondition failure rather than a duplicate-key error, which is what
        // lets the same statement serve a partially-succeeding batch.
        if single(self.writes.commit(vec![write]).await?)?.is_already_exists() {
            return Err(EntityError::already_exists(id));
        }

        self.emit_change(
            ChangeEvent::builder()
                .entity_id(entity.id.clone()) // clone: owned Uri for event
                .entity_types(types)
                .trigger_reason(NotificationTrigger::EntityCreated)
                .occurred_at(Utc::now())
                .origin(ChangeOrigin::Local)
                .changed_attributes(changed_attrs)
                .current_payload(Some(payload))
                .build(),
        );

        Ok(id.clone()) // clone: the caller owns the created identifier while the request entity remains borrowed
    }

    /// Merge a partial update into an existing entity (NGSI-LD 5.6.17).
    ///
    /// The merge is computed from a payload read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
    pub async fn merge(&self, id: &Uri, patch: &EntityPatch, context: &ContextSpec) -> Result<(), EntityError> {
        let entity_uri = id.clone(); // clone: mutation events and computed-write closures own the identifier

        let mut patch_json = serde_json::to_value(patch).map_err(|err| EntityError::validation(format!("Invalid patch payload: {}", err)))?;
        normalize_merge_fragment(&mut patch_json);
        let patch_expanded = Arc::new(self.engine.expand(patch_json, context).await?);
        validate_attribute_envelopes(&patch_expanded)?;
        validate_geo_property_geometries(&patch_expanded)?;

        let MergedEntity {
            merged,
            types,
            changed_attrs,
            deleted_attrs,
            previous_attributes,
        } = self
            .commit_computed_write(&entity_uri, |previous| {
                let attrs_before: HashSet<AttributeName> = extract_attribute_keys(previous).into_iter().collect();
                let previous_attributes = self.change_sink.as_ref().map(|_| previous.clone()); // clone: snapshot before mutation for showChanges diff

                let mut existing = previous.clone(); // clone: the merge mutates its own copy of the stored payload
                merge_entity(&mut existing, &patch_expanded)?;

                let merged = Arc::new(existing);
                let types = extract_types(&merged);
                let changed_attrs = extract_attribute_keys(&patch_expanded);
                let attrs_after: HashSet<AttributeName> = extract_attribute_keys(&merged).into_iter().collect();
                let deleted_attrs: Vec<AttributeName> = attrs_before.difference(&attrs_after).cloned().collect(); // clone: owned names from set difference

                let mut subscription_matches = Vec::new();
                for deleted_attr in &deleted_attrs {
                    subscription_matches.extend(self.build_subscription_matches(
                        &entity_uri,
                        &types,
                        NotificationTrigger::AttributeDeleted,
                        from_ref(deleted_attr),
                        Some(&merged),
                        previous_attributes.as_ref(),
                    ));
                }
                subscription_matches.extend(self.build_subscription_matches(
                    &entity_uri,
                    &types,
                    NotificationTrigger::EntityUpdated,
                    &changed_attrs,
                    Some(&merged),
                    previous_attributes.as_ref(),
                ));

                let history = mutation_history(&patch_expanded, previous, &merged);

                let write = AtomicWrite::builder()
                    .entity_id(Arc::new(entity_uri.clone())) // clone: the computed write owns its validated identifier.
                    .entity(EntityMutation::Update(EntityRecord::from_expanded(Arc::clone(&merged)))) // clone: Arc ref-count bump
                    .history(history)
                    .subscription_matches(subscription_matches)
                    .build();

                Ok((
                    write,
                    MergedEntity {
                        merged,
                        types,
                        changed_attrs,
                        deleted_attrs,
                        previous_attributes,
                    },
                ))
            })
            .await?;

        for deleted_attr in &deleted_attrs {
            self.emit_attr_change(
                entity_uri.clone(), // clone: value needed by subsequent loop iterations and final emit
                types.clone(),      // clone: value needed by subsequent loop iterations and final emit
                NotificationTrigger::AttributeDeleted,
                vec![deleted_attr.clone()],  // clone: owned copy from borrowed iterator
                previous_attributes.clone(), // clone: value needed by subsequent loop iterations and final emit
                Some(Arc::clone(&merged)),   // clone: Arc ref-count bump
            );
        }

        self.emit_attr_change(
            entity_uri,
            types,
            NotificationTrigger::EntityUpdated,
            changed_attrs,
            previous_attributes,
            Some(merged),
        );

        Ok(())
    }

    /// Replace an entire entity with the given payload (NGSI-LD 5.6.18).
    ///
    /// The replacement drops every attribute it omits, so each dropped instance
    /// needs a tombstone, and that diff is only meaningful against the payload
    /// the entity actually had when the write landed. The write therefore
    /// states the `modifiedAt` its diff assumed; a writer that got in first
    /// makes the store refuse it, and the entity is read again.
    pub async fn replace_entity(&self, id: &Uri, patch: &EntityPatch, context: &ContextSpec) -> Result<(), EntityError> {
        let entity_uri = id.clone(); // clone: mutation events and computed-write closures own the identifier
        // Read before expanding so a request against a missing entity reports
        // 404 rather than a payload-validation error.
        if self.reader.get_snapshot(&entity_uri).await?.is_none() {
            return Err(EntityError::not_found(&entity_uri));
        }

        let mut patch_json = serde_json::to_value(patch).map_err(|err| EntityError::validation(format!("Invalid patch payload: {}", err)))?;
        normalize_entity_attributes(&mut patch_json);
        let expanded = self.engine.expand(patch_json, context).await?;
        validate_attribute_envelopes(&expanded)?;
        validate_geo_property_geometries(&expanded)?;

        let payload = Arc::new(expanded);
        let changed_attrs = extract_attribute_keys(&payload);
        let entity_id = Arc::new(entity_uri.clone()); // clone: repeated computed-write attempts share the validated identifier.

        // The types the notification carries are the stored ones, read off the
        // payload the replacement actually superseded rather than off an earlier
        // attempt's.
        let types = self
            .commit_computed_write(&entity_uri, |previous| {
                let types = extract_types(previous);
                let history = mutation_history(&payload, previous, &payload);
                let subscription_matches =
                    self.build_subscription_matches(&entity_uri, &types, NotificationTrigger::EntityUpdated, &changed_attrs, Some(&payload), None);
                let write = AtomicWrite::builder()
                    .entity_id(Arc::clone(&entity_id)) // clone: Arc ref-count bump; the identifier outlives the attempt
                    .entity(EntityMutation::Update(EntityRecord::from_expanded(Arc::clone(&payload)))) // clone: Arc ref-count bump
                    .history(history)
                    .subscription_matches(subscription_matches)
                    .build();
                Ok((write, types))
            })
            .await?;

        self.emit_change(
            ChangeEvent::builder()
                .entity_id(entity_uri)
                .entity_types(types)
                .trigger_reason(NotificationTrigger::EntityUpdated)
                .occurred_at(Utc::now())
                .origin(ChangeOrigin::Local)
                .changed_attributes(changed_attrs)
                .current_payload(Some(payload))
                .build(),
        );
        Ok(())
    }

    /// Delete an entity by its URI (NGSI-LD 5.6.6).
    pub async fn delete(&self, id: &Uri) -> Result<(), EntityError> {
        let entity_uri = id.clone(); // clone: delete events and persistence work own the identifier
        let id = id.as_str();

        let entity_snapshot = self.reader.get(&entity_uri).await?.ok_or_else(|| EntityError::not_found(&entity_uri))?;
        let entity_types = extract_types(&entity_snapshot);

        // Captured before the delete so the notification can carry them: once
        // the row is gone they are unreadable.
        let sys_attrs_snapshot = match self.reader.get_sys_attrs(&entity_uri).await {
            Ok(attrs) => attrs,
            Err(e) => {
                tracing::warn!("Failed to fetch sys_attrs before delete for {}: {}", id, e);
                None
            }
        };

        let attr_keys = extract_attribute_keys(&entity_snapshot);

        let mut subscription_matches = Vec::new();
        if !attr_keys.is_empty() {
            subscription_matches.extend(self.build_subscription_matches(
                &entity_uri,
                &entity_types,
                NotificationTrigger::AttributeDeleted,
                &attr_keys,
                Some(&entity_snapshot),
                Some(&entity_snapshot),
            ));
        }
        subscription_matches.extend(self.build_subscription_matches(
            &entity_uri,
            &entity_types,
            NotificationTrigger::EntityDeleted,
            &[],
            Some(&entity_snapshot),
            Some(&entity_snapshot),
        ));

        let write = AtomicWrite::builder()
            .entity_id(Arc::new(entity_uri.clone())) // clone: the delete write owns its validated identifier.
            .entity(EntityMutation::Delete)
            // Clause 5.6.6 removes current state and leaves the Temporal
            // Evolution standing, so every instance the entity still held is
            // recorded as ended rather than being deleted with it.
            .history(deletion_history(&entity_snapshot))
            .subscription_matches(subscription_matches)
            .precondition(Precondition::MustExist)
            .build();

        if single(self.writes.commit(vec![write]).await?)?.is_not_found() {
            return Err(EntityError::not_found(&entity_uri));
        }

        // One event per deleted attribute so `attributeDeleted` subscriptions fire.
        if !attr_keys.is_empty() {
            self.emit_change(
                ChangeEvent::builder()
                    .entity_id(entity_uri.clone()) // clone: value also needed by subsequent EntityDeleted emit
                    .entity_types(entity_types.clone()) // clone: value also needed by subsequent EntityDeleted emit
                    .trigger_reason(NotificationTrigger::AttributeDeleted)
                    .occurred_at(Utc::now())
                    .origin(ChangeOrigin::Local)
                    .changed_attributes(attr_keys)
                    .entity_snapshot(Some(entity_snapshot.clone())) // clone: value also needed by subsequent EntityDeleted emit
                    .previous_attributes(Some(entity_snapshot.clone())) // clone: value also needed by subsequent EntityDeleted emit
                    .sys_attrs_snapshot(sys_attrs_snapshot.clone()) // clone: value also needed by subsequent EntityDeleted emit
                    .build(),
            );
        }

        self.emit_change(
            ChangeEvent::builder()
                .entity_id(entity_uri)
                .entity_types(entity_types)
                .trigger_reason(NotificationTrigger::EntityDeleted)
                .occurred_at(Utc::now())
                .origin(ChangeOrigin::Local)
                .entity_snapshot(Some(entity_snapshot.clone())) // clone: separate ownership for snapshot and previous fields
                .previous_attributes(Some(entity_snapshot))
                .sys_attrs_snapshot(sys_attrs_snapshot)
                .build(),
        );

        Ok(())
    }
}
