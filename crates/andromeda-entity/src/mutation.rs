//! Whole-entity writes: create, merge, replace and delete (clauses 5.6.1,
//! 5.6.17, 5.6.18 and 5.6.6).
//!
//! Attribute-level mutations live in `attrs.rs`; batch forms live in `batch/`.

use crate::{
    EntityService,
    entity_validation_reason::EntityValidationReason,
    error::EntityError,
    merge_fragment::{adapt_member, existing_attribute_types},
    merge_options::MergeOptions,
    notification_fanout::{ChangedEntity, MutationSnapshots},
    notification_plan::{NotificationGroup, attribute_change_groups},
    sys_attrs::snapshot_with_sys_attrs,
    write_admission::{EntityWrite, admit_expanded_entity},
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    change_event::ChangeEvent,
    entity_mutation::EntityMutation,
    entity_record::EntityRecord,
    history_record::{HistoryRecord, deletion_history, mutation_history},
    precondition::Precondition,
    write_outcome::single,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, engine::term_lookup::PreparedTermContext, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    entity::{Entity, EntityPatch},
    expanded::{
        attribute_change::classify_attribute_changes,
        attribute_system_time::stamp_attribute_instants,
        entity_member::{extract_attribute_names, extract_types},
        mutation::entity::merge_entity,
        node::expanded_object_ref,
    },
    notification::{ChangeOrigin, NotificationTrigger},
};
use andromeda_ngsi_ld_query::type_selection::{expansion::expand_type_expression, grammar::TypeExpr};
use andromeda_ngsi_ld_representation::{
    entity_members::ENTITY_MEMBERS,
    merge_representation::MergeRepresentation,
    normalization::{normalize_entity_attributes, normalize_merge_fragment},
};
use andromeda_telemetry::entity::{self, EntityOperation};
use chrono::Utc;
use serde_json::Value;
use std::{slice, sync::Arc};

/// Whether an expanded entity's types satisfy a clause 4.17 selector.
fn entity_satisfies(expanded: &Value, selector: &TypeExpr) -> bool {
    let types = expanded_object_ref(expanded)
        .and_then(|object| object.get("@type"))
        .and_then(Value::as_array)
        .map(|types| types.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    selector.matches(&types)
}

/// What one clause 5.6.17 merge produced, beyond the write itself.
///
/// Read off the attempt that actually landed: the merge runs again whenever a
/// concurrent writer supersedes an attempt, and the notification must describe
/// the change the store accepted rather than one that was computed and lost.
struct MergedEntity {
    /// In-process events matching the durable records committed with the write.
    events: Vec<ChangeEvent>,
}

impl EntityService {
    /// Create a new NGSI-LD entity, returning its URI on success (clause 5.6.1).
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Validation`] when the entity cannot be serialised
    /// or carries a malformed attribute instance, a null value or an invalid
    /// `GeoProperty` geometry, [`EntityError::JsonLd`] when it cannot be
    /// expanded against `context`, [`EntityError::AlreadyExists`] when another
    /// entity already holds the identifier (clause 6.4.3.1), and
    /// [`EntityError::Persistence`] when the store could not commit the insert.
    pub async fn create(&self, target: &ContextTarget, entity: &Entity, context: &ContextSpec) -> Result<Uri, EntityError> {
        let timer = entity::start(EntityOperation::Create);
        let result: Result<Uri, EntityError> = async {
            let id = &entity.id;

            let mut entity_json = serde_json::to_value(entity).map_err(|err| EntityError::Validation(EntityValidationReason::EntityPayload(err)))?;
            normalize_entity_attributes(&mut entity_json);
            let expanded = self.engine.expand(entity_json, context).await?;
            // Clause 5.6.1.4 opens with the clause 5.5.4 validation. The transport
            // layer runs the same screening before it forwards anything, and runs
            // it again here on whatever the forwarding phase left behind.
            admit_expanded_entity(&expanded, EntityWrite::Create.null_sentinel_admission())?;

            // Clause 4.8: a created entity's attribute instances carry the write
            // instant as both createdAt and modifiedAt.
            let mut expanded = expanded;
            stamp_attribute_instants(&mut expanded, &Value::Null, Utc::now());
            let payload = Arc::new(expanded);
            let types = extract_types(&payload);
            let changed_attrs = extract_attribute_names(&payload);
            let mut groups = vec![NotificationGroup::entity(NotificationTrigger::EntityCreated, changed_attrs)];
            groups.extend(attribute_change_groups(&Value::Null, &payload));
            let plan = self.plan_notifications(target, &entity.id, &types, &groups, Some(&payload), None);

            let write = AtomicWrite::builder()
                .entity_id(Arc::new(id.clone())) // clone: the write owns a shared validated identifier.
                .entity(EntityMutation::Insert(EntityRecord::from_expanded(Arc::clone(&payload)))) // clone: Arc ref-count bump; the payload is also the change event's
                .history(vec![HistoryRecord::Observations(Arc::clone(&payload))]) // clone: Arc ref-count bump
                .subscription_matches(plan.matches)
                .precondition(Precondition::MustNotExist)
                .build();

            // A conflicting insert is 409, per NGSI-LD 6.4.3.1. It reaches here as a
            // precondition failure rather than a duplicate-key error, which is what
            // lets the same statement serve a partially-succeeding batch.
            if single(self.writes.commit(target, vec![write]).await?)?.is_already_exists() {
                return Err(EntityError::already_exists(id));
            }

            plan.events.into_iter().for_each(|event| self.emit_change(target, event));

            Ok(id.clone()) // clone: the caller owns the created identifier while the request entity remains borrowed
        }
        .await;
        timer.finish_result(&result);
        result
    }

    /// Merge a partial update into an existing entity (NGSI-LD 5.6.17) under
    /// the Table 6.5.3.4-1 options the request named.
    ///
    /// The merge is computed from a payload read outside the writing
    /// transaction, so the write states the `modifiedAt` it assumed and is
    /// recomputed against a fresh read if a concurrent writer got in first.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Validation`] when the patch cannot be serialised,
    /// cannot be adapted to the attribute families the target holds, or sets a
    /// `datasetId` to the NGSI-LD Null (clause 5.5.12),
    /// [`EntityError::JsonLd`] when the patch or the type expression cannot be
    /// expanded against `context`, [`EntityError::NotFound`] when no entity
    /// carries `id` or the type selector excludes it,
    /// [`EntityError::Contention`] when concurrent writers superseded the
    /// computed write on every attempt, and [`EntityError::Persistence`] when
    /// the store could not apply it.
    pub async fn merge(
        &self,
        target: &ContextTarget,
        id: &Uri,
        patch: &EntityPatch,
        context: &ContextSpec,
        options: MergeOptions<'_>,
    ) -> Result<(), EntityError> {
        let timer = entity::start(EntityOperation::Merge);
        let result: Result<(), EntityError> = async {
            let entity_uri = id.clone(); // clone: mutation events and computed-write closures own the identifier

            let mut patch_json = serde_json::to_value(patch).map_err(|err| EntityError::Validation(EntityValidationReason::PatchPayload(err)))?;
            let prepared_context = self.engine.prepare_term_context(context);
            let type_selector = match options.type_selector {
                Some(raw) => Some(expand_type_expression(raw, &ContextualExpander::new(&self.engine, &prepared_context)).await?),
                None => None,
            };
            self.adapt_merge_fragment(target, id, &mut patch_json, &prepared_context, options).await?;
            normalize_merge_fragment(&mut patch_json);
            let patch_expanded = Arc::new(self.engine.expand(patch_json, context).await?);
            // Clause 5.5.12 lets a merge fragment carry the NGSI-LD Null at any
            // depth, but "a datasetId cannot be deleted by setting it to the value
            // urn:ngsi-ld:null". Clause 4.18 admits the sentinel as the whole value
            // of `scope`, which clause 5.5.12 then reads as the deletion of that
            // member.
            admit_expanded_entity(&patch_expanded, EntityWrite::MergeEntity.null_sentinel_admission())?;

            let now = Utc::now();
            let MergedEntity { events } = self
                .commit_computed_write(target, &entity_uri, |previous| {
                    // Clause 5.6.17.4: the target is the entity with this id "and,
                    // where specified, type"; one outside the selector is not it.
                    if type_selector.as_ref().is_some_and(|selector| !entity_satisfies(previous, selector)) {
                        return Err(EntityError::not_found(&entity_uri));
                    }
                    let mut existing = previous.clone(); // clone: the merge mutates its own copy of the stored payload
                    merge_entity(&mut existing, &patch_expanded, options.observed_at)?;

                    // Clause 4.8: a merged instance keeps its createdAt and, when its
                    // content changed, takes `now` for modifiedAt.
                    stamp_attribute_instants(&mut existing, previous, now);
                    let merged = Arc::new(existing);
                    let types = extract_types(&merged);
                    let groups = attribute_change_groups(previous, &merged);
                    let plan = self.plan_notifications(target, &entity_uri, &types, &groups, Some(&merged), Some(previous));

                    let history = mutation_history(&patch_expanded, previous, &merged);

                    let write = AtomicWrite::builder()
                        .entity_id(Arc::new(entity_uri.clone())) // clone: the computed write owns its validated identifier.
                        .entity(EntityMutation::Update(EntityRecord::from_expanded(Arc::clone(&merged)))) // clone: Arc ref-count bump
                        .history(history)
                        .subscription_matches(plan.matches)
                        .build();

                    Ok((write, MergedEntity { events: plan.events }))
                })
                .await?;

            for event in events {
                self.emit_change(target, event);
            }

            Ok(())
        }
        .await;
        timer.finish_result(&result);
        result
    }

    /// Rewrite a merge fragment for the attribute types the target already
    /// holds (Table 6.5.3.4-1 `format=simplified` and `lang`).
    ///
    /// A normalized fragment without `lang` needs no rewrite, so the target is
    /// not read for it. A target that does not exist is left for the write to
    /// report as not found.
    async fn adapt_merge_fragment(
        &self,
        target: &ContextTarget,
        id: &Uri,
        fragment: &mut Value,
        prepared_context: &PreparedTermContext,
        options: MergeOptions<'_>,
    ) -> Result<(), EntityError> {
        if options.representation == MergeRepresentation::Normalized && options.lang.is_none() {
            return Ok(());
        }
        let Some(members) = fragment.as_object_mut() else {
            return Ok(());
        };
        let Some(existing) = self.reader.get(target, id).await? else {
            return Ok(());
        };
        let existing_types = expanded_object_ref(&existing).map(existing_attribute_types).unwrap_or_default();
        for (member, value) in members.iter_mut() {
            if ENTITY_MEMBERS.contains(&member.as_str()) {
                continue;
            }
            let iri = self.engine.expand_term(member, prepared_context).await?;
            adapt_member(member, value, existing_types.get(iri.as_str()).copied(), options.representation, options.lang)
                .map_err(|error| EntityError::Validation(EntityValidationReason::MemberAdaptation(error)))?;
        }
        Ok(())
    }

    /// Replace an entire entity with the given payload (NGSI-LD 5.6.18).
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
    /// [`EntityError::Validation`] when the payload cannot be serialised or
    /// carries a malformed attribute instance, a null value (clause 5.6.18.4)
    /// or an invalid `GeoProperty` geometry, [`EntityError::JsonLd`] when it
    /// cannot be expanded against `context`, [`EntityError::Contention`] when
    /// concurrent writers superseded the computed write on every attempt, and
    /// [`EntityError::Persistence`] when the store could not apply it.
    pub async fn replace_entity(&self, target: &ContextTarget, id: &Uri, patch: &EntityPatch, context: &ContextSpec) -> Result<(), EntityError> {
        let timer = entity::start(EntityOperation::Replace);
        let result: Result<(), EntityError> = async {
            let entity_uri = id.clone(); // clone: mutation events and computed-write closures own the identifier
            // Read before expanding so a request against a missing entity reports
            // 404 rather than a payload-validation error.
            if self.reader.get_snapshot(target, &entity_uri).await?.is_none() {
                return Err(EntityError::not_found(&entity_uri));
            }

            let mut patch_json = serde_json::to_value(patch).map_err(|err| EntityError::Validation(EntityValidationReason::PatchPayload(err)))?;
            normalize_entity_attributes(&mut patch_json);
            let expanded = self.engine.expand(patch_json, context).await?;
            // Clause 5.6.18.4: Replace Entity does not support null values.
            admit_expanded_entity(&expanded, EntityWrite::ReplaceEntity.null_sentinel_admission())?;

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
                    stamp_attribute_instants(&mut replaced, previous, now);
                    let payload = Arc::new(replaced);
                    let types = extract_types(&payload);
                    let history = mutation_history(&payload, previous, &payload);
                    let groups = attribute_change_groups(previous, &payload);
                    let plan = self.plan_notifications(target, &entity_uri, &types, &groups, Some(&payload), Some(previous));
                    let write = AtomicWrite::builder()
                        .entity_id(Arc::clone(&entity_id)) // clone: Arc ref-count bump; the identifier outlives the attempt
                        .entity(EntityMutation::Update(EntityRecord::from_expanded(Arc::clone(&payload)))) // clone: Arc ref-count bump
                        .history(history)
                        .subscription_matches(plan.matches)
                        .build();
                    Ok((write, plan.events))
                })
                .await?;

            for event in events {
                self.emit_change(target, event);
            }
            Ok(())
        }
        .await;
        timer.finish_result(&result);
        result
    }

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
            let entity_types = extract_types(&entity_snapshot);

            // Captured before the delete so the notification can carry them: once
            // the row is gone they are unreadable.
            let sys_attrs_snapshot = match self.reader.get_sys_attrs(target, &entity_uri).await {
                Ok(attrs) => attrs,
                Err(e) => {
                    tracing::warn!("Failed to fetch sys_attrs before delete for {}: {}", id, e);
                    None
                }
            };

            let attr_keys = extract_attribute_names(&entity_snapshot);
            let attribute_changes = classify_attribute_changes(&entity_snapshot, &Value::Null);

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
                .history(deletion_history(&entity_snapshot))
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
