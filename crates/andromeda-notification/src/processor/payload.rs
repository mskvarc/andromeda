//! Notification payload building: showChanges, previous values, datasetId
//! filtering, nullification, projection, and format rendering.

use crate::processor::payload::{
    cache::{NotificationCompactionCache, ResolvedLinkedEntities, resolve_linked_cached},
    context::{normalize_compacted_entity, strip_to_structural_keys},
    deletion::{fix_null_language_maps, inject_deleted_attributes, nullify_entity_attributes},
    filtering::{filter_entity_attributes, filter_entity_by_dataset_id, inject_previous_values},
    projection::apply_notification_projection,
    representation::{compact_for_notification, render_format},
};
use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    query::EntityQueryService,
    sys_attrs::{expanded_entity_object_mut, inject_attr_sys_attrs},
};
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{
        context_compactor::ContextCompactor,
        options::{CompactOptions, ContextStripping},
        processor::JsonLdEngine,
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    entity::Entity,
    expanded::{attribute_expiry::remove_expired_instances, attribute_system_time::strip_attribute_instants},
    notification::NotificationTrigger,
};
use andromeda_ngsi_ld_number::json_value::deserialize_exact;
use andromeda_ngsi_ld_query::projection::term::ProjectionTerm;
use andromeda_ngsi_ld_render::{
    notification_conformance::amend_notification_entities,
    snapshot::SnapshotRenderer,
    structural_projection::StructuralProjection,
    uri_decoding::decode_percent_encoded_uris,
};
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, sys_attrs::SysAttrsInclusion};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use andromeda_subscription::compiled::CompiledSubscription;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::mem::take;
use tracing::debug;

/// Whether the entity the notification is about outlived the change.
///
/// Clause 5.8.6 shapes a notification "triggered by the deletion of an Entity"
/// down to `deletedAt` alone, plus `createdAt` and `modifiedAt` under
/// `sysAttrs`. Deleting an entity raises one `AttributeDeleted` event per
/// attribute as well as the `EntityDeleted` one, so a subscription watching an
/// attribute is notified through the attribute trigger and the trigger alone
/// cannot say whether the entity is still there. This does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum EntitySurvival {
    /// The entity outlived the change; a deleted attribute is reported against
    /// the entity that survived it.
    Survived,
    /// The entity itself is gone.
    Deleted,
}

/// Whether `event` leaves the entity behind.
///
/// An `EntityDeleted` event says so outright. An `AttributeDeleted` event says
/// so by carrying a pre-deletion snapshot and no post-mutation payload, which
/// is the shape both the immediate path (`EntityService`'s delete raises one
/// such event per attribute) and the durable path replay produce; an ordinary
/// attribute deletion carries the entity that survived it instead.
pub(super) const fn entity_survival(event: &ChangeEvent) -> EntitySurvival {
    match event.trigger_reason {
        NotificationTrigger::EntityDeleted => EntitySurvival::Deleted,
        NotificationTrigger::AttributeDeleted => {
            if event.current_payload.is_none() && event.entity_snapshot.is_some() {
                EntitySurvival::Deleted
            } else {
                EntitySurvival::Survived
            }
        }
        NotificationTrigger::EntityCreated
        | NotificationTrigger::EntityUpdated
        | NotificationTrigger::AttributeCreated
        | NotificationTrigger::AttributeUpdated => EntitySurvival::Survived,
    }
}

pub(super) struct NotificationBuildParams<'a> {
    pub(super) target: &'a ContextTarget,
    pub(super) engine: &'a JsonLdEngine,
    pub(super) entity_data: &'a Value,
    pub(super) previous_attributes: Option<&'a Value>,
    pub(super) sub: &'a CompiledSubscription,
    pub(super) trigger_reason: &'a NotificationTrigger,
    pub(super) survival: EntitySurvival,
    pub(super) changed_attributes: &'a [AttributeName],
    pub(super) changed_dataset_id: Option<&'a Uri>,
    pub(super) occurred_at: DateTime<Utc>,
    pub(super) entity_query: &'a EntityQueryService,
    pub(super) compaction_cache: Option<&'a NotificationCompactionCache>,
}

/// The entities one notification carries for a subscription (clause 5.8.6).
///
/// The entity the change event supplies is shaped for its trigger, reduced to
/// what the subscription selects, compacted under the subscription's context,
/// given its `showChanges` view and its `join` linked entities, and finally
/// rendered in the subscription's `notification.format`. An entity that is
/// gone by the time any of that runs leaves nothing to notify, which is an
/// empty payload.
pub(super) async fn build_notification_data(params: &NotificationBuildParams<'_>) -> Vec<Value> {
    let mut entity = expanded_entity_node(params.entity_data);
    if entity.is_null() {
        return vec![];
    }
    // Clause 5.8.6 notifies the entity as it is, and clause 4.22 makes an
    // instance past its `expiresAt` invalid, so an expired instance the sweep
    // has yet to delete is not part of it, nor of the `attributes` narrowing
    // and projection below.
    remove_expired_instances(&mut entity, Utc::now());

    apply_deletion_shape(&mut entity, params);
    apply_notification_selection(&mut entity, params.sub);

    // Clause 4.8 with Table 5.2.14-1: the change payload carries every attribute
    // instance's createdAt/modifiedAt, so a subscription that did not ask for
    // `sysAttrs` has them removed before the entity is compacted and rendered.
    if params.sub.notification_sys_attrs == SysAttrsInclusion::Strip {
        strip_attribute_instants(&mut entity);
    }

    let context = ContextSpec::for_subscription_context(params.sub.jsonld_context.as_ref());
    let compactor = params.engine.compactor(&context);
    let Ok(mut compacted) = compact_for_notification(&compactor, &entity, params.compaction_cache).await else {
        return vec![];
    };
    // Clause 4.21, the `id` / `type` / `scope` half: the attribute half of the
    // projection ran above on the expanded entity, keyed by attribute IRI; the
    // three structural members only exist as such once compaction has turned
    // `@id` and `@type` into them, so their half of the rule runs here -- the
    // same rule, and the same code, a query response applies. Clause 5.8.6 holds
    // `id` back from that rule: a notification carries Entities and an Entity is
    // only actionable with its `id` (clause 4.5.1), so `id` is captured and
    // restored around the prune while `type` and `scope` follow the projection.
    // clone: the id is captured before the prune that may drop it, then restored.
    let notified_id = compacted.get(ProjectionTerm::ID_KEYWORD).cloned();
    StructuralProjection::from(&params.sub.notification_projection).prune(&mut compacted);
    if let (Some(id), Some(map)) = (notified_id, compacted.as_object_mut()) {
        map.entry(ProjectionTerm::ID_KEYWORD.to_owned()).or_insert(id);
    }
    apply_show_changes(&mut compacted, params, &compactor).await;

    let payload = apply_join(compacted, params, &context).await;
    render_notification_entities(payload, params.sub)
}

/// The single expanded entity node a notification is built from.
///
/// An expanded entity travels either bare or wrapped in a one-element array; a
/// document carrying neither yields [`Value::Null`], which the caller reads as
/// nothing left to notify about.
fn expanded_entity_node(document: &Value) -> Value {
    match document {
        Value::Array(values) => match values.first() {
            // clone: the notification owns its node while the event keeps the document
            Some(value) => value.clone(),
            None => Value::Null,
        },
        // clone: same
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => other.clone(),
    }
}

/// Shape the notified entity for a deletion trigger (clause 5.8.6).
///
/// A deleted entity is carried as its structural members alone, or, under
/// `showChanges`, with every attribute nullified so the subscriber sees what
/// went. A deleted attribute is materialised back into what survived it. Every
/// other trigger notifies the entity as the write left it.
fn apply_deletion_shape(entity: &mut Value, params: &NotificationBuildParams<'_>) {
    // Clause 5.8.6 keys this on what the notification was triggered by, not on
    // which clause 5.2.12 trigger matched the subscription: an entity deletion
    // reaches an `attributeDeleted` subscriber too, and it owes that subscriber
    // the same `deletedAt`-only document.
    if params.survival == EntitySurvival::Deleted {
        if params.sub.notification_show_changes {
            nullify_entity_attributes(entity);
        } else {
            let structural = strip_to_structural_keys(entity, params.sub.notification_sys_attrs);
            *entity = structural;
        }
        return;
    }
    match params.trigger_reason {
        NotificationTrigger::AttributeDeleted => apply_attribute_deletion(entity, params),
        NotificationTrigger::EntityDeleted
        | NotificationTrigger::EntityCreated
        | NotificationTrigger::EntityUpdated
        | NotificationTrigger::AttributeCreated
        | NotificationTrigger::AttributeUpdated => (),
    }
}

/// Materialise the attribute instances a deletion removed, so the notification
/// carries what was deleted and not only what remains.
///
/// A subscription with `watchedAttributes` is told about those attributes
/// alone, so the changed set is narrowed to them before the instances are
/// injected.
fn apply_attribute_deletion(entity: &mut Value, params: &NotificationBuildParams<'_>) {
    let sub = params.sub;
    let deleted_at = if sub.notification_sys_attrs == SysAttrsInclusion::Include {
        Some(params.occurred_at)
    } else {
        None
    };
    let watched_changed: Vec<AttributeName>;
    let effective_changed: &[AttributeName] = if let Some(ref watched) = sub.watched_attrs {
        watched_changed = params
            .changed_attributes
            .iter()
            .filter(|attribute| watched.contains(*attribute))
            .cloned()
            .collect();
        &watched_changed
    } else {
        params.changed_attributes
    };
    inject_deleted_attributes(entity, effective_changed, params.changed_dataset_id, params.previous_attributes, deleted_at);

    if sub.notification_sys_attrs == SysAttrsInclusion::Include
        && let Some(object) = expanded_entity_object_mut(entity)
    {
        // clone: the entity-level timestamps are read out before the same object is mutated
        let created_at = object.get(expanded_properties::CREATED_AT.as_str()).cloned();
        let modified_at = object.get(expanded_properties::MODIFIED_AT.as_str()).cloned();
        if let (Some(created_at), Some(modified_at)) = (created_at, modified_at) {
            inject_attr_sys_attrs(object, &created_at, &modified_at);
        }
    }
}

/// Reduce the notified entity to what the subscription selects: the
/// `datasetId` instances it names, the attributes it lists, and its
/// `pick`/`omit` projection.
fn apply_notification_selection(entity: &mut Value, sub: &CompiledSubscription) {
    if let Some(ref dataset_ids) = sub.dataset_id {
        filter_entity_by_dataset_id(entity, dataset_ids);
    }
    if let Some(ref attributes) = sub.notification_attributes {
        let selected = filter_entity_attributes(entity, attributes);
        *entity = selected;
    }
    // Clause 5.8.6 reduces "every Entity within the payload body ... down to
    // only contain the defined entity members listed", which is clause
    // 5.7.1.5's sentence for a query response. Only the attribute half is
    // answerable here, on the expanded entity; `id`, `type` and `scope` are
    // pruned after compaction.
    apply_notification_projection(entity, &sub.notification_projection);
}

/// Apply the `showChanges` view to the compacted entity (clause 5.8.6): the
/// language maps a deletion left null, and the previous value of every changed
/// instance.
///
/// A previous representation that cannot be compacted leaves the notification
/// with its current values alone rather than failing the delivery.
async fn apply_show_changes(compacted: &mut Value, params: &NotificationBuildParams<'_>, compactor: &ContextCompactor<'_>) {
    if !params.sub.notification_show_changes {
        return;
    }
    if matches!(params.trigger_reason, NotificationTrigger::EntityDeleted) {
        fix_null_language_maps(compacted);
    }
    let Some(previous) = params.previous_attributes else {
        return;
    };
    let Ok(previous_compacted) = compactor
        .compact(
            expanded_entity_node(previous),
            CompactOptions {
                strip_context: ContextStripping::Strip,
            },
        )
        .await
    else {
        return;
    };
    let previous_compacted = normalize_compacted_entity(previous_compacted);
    let exact_attribute_instance = matches!(
        params.trigger_reason,
        NotificationTrigger::AttributeCreated | NotificationTrigger::AttributeUpdated | NotificationTrigger::AttributeDeleted
    );
    inject_previous_values(
        compacted,
        &previous_compacted,
        params.changed_attributes,
        params.changed_dataset_id,
        exact_attribute_instance,
        params.engine,
        compactor.context(),
    )
    .await;
}

/// Place the linked entities the subscription's `join` asks for into the
/// payload (clause 4.5.23): appended beside the notified entity for `flat`,
/// nested inside it for `inline`.
async fn apply_join(compacted: Value, params: &NotificationBuildParams<'_>, context: &ContextSpec) -> Vec<Value> {
    let mut payload = vec![compacted];
    match params.sub.notification_join {
        Some(JoinMode::Flat) => {
            let resolved = resolve_join_targets(&payload[0], params, context).await;
            if let Some(resolved) = resolved {
                payload.reserve(resolved.as_values.len());
                // clone: the notification owns its payload while the resolution stays cached for the notifications still to come
                payload.extend(resolved.as_values.iter().cloned());
            }
        }
        Some(JoinMode::Inline) => {
            let resolved = resolve_join_targets(&payload[0], params, context).await;
            if let Some(resolved) = resolved {
                let notified = take(&mut payload[0]);
                match SnapshotRenderer::new(notified).with_inline_linked_entities(&resolved.by_id).render() {
                    Ok(rendered) => payload[0] = rendered,
                    Err(error) => debug!(%error, "Failed to apply join=inline to a notification payload"),
                }
            }
        }
        Some(JoinMode::None) | None => (),
    }
    payload
}

/// The linked entities of the notified entity, or `None` when the payload
/// cannot be read back as an entity or its links cannot be resolved.
///
/// Neither condition fails the notification: it goes out carrying the notified
/// entity alone.
async fn resolve_join_targets(notified: &Value, params: &NotificationBuildParams<'_>, context: &ContextSpec) -> Option<ResolvedLinkedEntities> {
    let main_entity = deserialize_exact::<Entity>(notified).ok()?;
    match resolve_linked_cached(
        params.target,
        params.entity_query,
        &main_entity,
        params.sub.notification_join_level,
        params.sub.notification_sys_attrs,
        context,
        params.compaction_cache,
    )
    .await
    {
        Ok(resolved) => Some(resolved),
        Err(error) => {
            debug!(%error, "Failed to resolve linked entities for a notification payload");
            None
        }
    }
}

/// Render every entity in the payload in the subscription's
/// `notification.format` (clause 5.8.6), then amend it to the subscription's
/// declared `ngsildConformance`.
///
/// Clause 5.8.6 puts the amendment last, after the format rendering and after
/// `pick`, `omit` and `join`, so the fallback tables of clause 4.3.6.8 run over
/// the representation the subscriber will actually receive.
fn render_notification_entities(payload: Vec<Value>, sub: &CompiledSubscription) -> Vec<Value> {
    let format = sub.notification_format.unwrap_or_default();
    let rendered: Vec<Value> = payload
        .into_iter()
        .map(|entity| {
            let mut formatted = render_format(entity, format, sub.notification_join);
            // The same targeted walk the query response uses (clause 4.5.1): only a
            // Relationship's `object` and a ListRelationship's `objectList` are
            // percent-encoded entity ids to decode, so a Property value carrying a
            // `%` is not corrupted, and a notification decodes exactly as a GET of
            // the same entity does.
            decode_percent_encoded_uris(&mut formatted);
            formatted
        })
        .collect();
    amend_notification_entities(rendered, sub.ngsild_conformance, format)
}

pub(super) mod cache;
/// Notification context selection and compacted-entity normalization.
pub mod context;
/// Deleted attribute and instance materialization.
mod deletion;
/// Notification filtering and previous-value materialization.
pub mod filtering;
/// The `pick`/`omit` projection applied to an expanded entity.
pub(crate) mod projection;
/// Compaction and format rendering shared by both notification paths.
pub(crate) mod representation;
