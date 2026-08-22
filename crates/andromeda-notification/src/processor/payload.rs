//! Notification payload building: showChanges, previous values, datasetId
//! filtering, nullification, projection, and format rendering.

use crate::processor::payload::{
    cache::{NotificationCompactionCache, resolve_linked_cached},
    context::{normalize_compacted_entity, strip_to_structural_keys},
    deletion::{fix_null_language_maps, inject_deleted_attributes, nullify_entity_attributes},
    filtering::{filter_entity_attributes, filter_entity_by_dataset_id, inject_previous_values},
    projection::apply_notification_projection,
    representation::{compact_for_notification, render_format},
};
use andromeda_entity::{
    query::EntityQueryService,
    sys_attrs::{expanded_entity_object_mut, inject_attr_sys_attrs},
};
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{
        options::{CompactOptions, ContextStripping},
        processor::JsonLdEngine,
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{entity::Entity, notification::NotificationTrigger};
use andromeda_ngsi_ld_number::json_value::deserialize_exact;
use andromeda_ngsi_ld_render::{snapshot::SnapshotRenderer, uri_decoding::decode_percent_encoded_uris};
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, sys_attrs::SysAttrsInclusion};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use andromeda_subscription::compiled::CompiledSubscription;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::mem::take;
use tracing::debug;

pub(super) struct NotificationBuildParams<'a> {
    pub(super) engine: &'a JsonLdEngine,
    pub(super) entity_data: &'a Value,
    pub(super) previous_attributes: Option<&'a Value>,
    pub(super) sub: &'a CompiledSubscription,
    pub(super) trigger_reason: &'a NotificationTrigger,
    pub(super) changed_attributes: &'a [AttributeName],
    pub(super) changed_dataset_id: Option<&'a Uri>,
    pub(super) occurred_at: DateTime<Utc>,
    pub(super) entity_query: &'a EntityQueryService,
    pub(super) compaction_cache: Option<&'a NotificationCompactionCache>,
}

pub(super) async fn build_notification_data(params: NotificationBuildParams<'_>) -> Vec<Value> {
    let NotificationBuildParams {
        engine,
        entity_data,
        previous_attributes,
        sub,
        trigger_reason,
        changed_attributes,
        changed_dataset_id,
        occurred_at,
        entity_query,
        compaction_cache,
    } = params;

    let mut entity = match entity_data {
        Value::Array(arr) => match arr.first() {
            Some(value) => value.clone(),
            None => Value::Null,
        },
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => other.clone(),
    };

    if entity.is_null() {
        return vec![];
    }

    let is_entity_deleted = matches!(trigger_reason, NotificationTrigger::EntityDeleted);
    let is_attr_deleted = matches!(trigger_reason, NotificationTrigger::AttributeDeleted);

    if is_entity_deleted && !sub.notification_show_changes {
        entity = strip_to_structural_keys(&entity, sub.notification_sys_attrs);
    }

    if is_entity_deleted && sub.notification_show_changes {
        nullify_entity_attributes(&mut entity);
    }

    if is_attr_deleted {
        let deleted_at = if sub.notification_sys_attrs == SysAttrsInclusion::Include {
            Some(occurred_at)
        } else {
            None
        };
        let filtered_changed: Vec<AttributeName>;
        let effective_changed = if let Some(ref watched) = sub.watched_attrs {
            filtered_changed = changed_attributes.iter().filter(|attribute| watched.contains(*attribute)).cloned().collect();
            &filtered_changed
        } else {
            changed_attributes
        };
        inject_deleted_attributes(&mut entity, effective_changed, changed_dataset_id, previous_attributes, deleted_at);

        if sub.notification_sys_attrs == SysAttrsInclusion::Include
            && let Some(obj) = expanded_entity_object_mut(&mut entity)
        {
            let ca = obj.get(expanded_properties::CREATED_AT.as_str()).cloned();
            let ma = obj.get(expanded_properties::MODIFIED_AT.as_str()).cloned();
            if let (Some(ca), Some(ma)) = (ca, ma) {
                inject_attr_sys_attrs(obj, &ca, &ma);
            }
        }
    }

    if let Some(ref dataset_ids) = sub.dataset_id {
        filter_entity_by_dataset_id(&mut entity, dataset_ids);
    }

    let mut entity = if let Some(ref attrs) = sub.notification_attributes {
        filter_entity_attributes(&entity, attrs)
    } else {
        entity
    };

    // Clause 5.8.6: `pick` and `omit` reduce every entity in the payload to
    // the listed members. `id` and `type` are never among what goes: clause
    // 5.3.1 carries Entities in `data`, and Table 5.2.4-1 makes both mandatory
    // (cardinality 1), so a notified entity that lost them would not be one.
    apply_notification_projection(&mut entity, &sub.notification_projection);

    let context = ContextSpec::for_subscription_context(sub.jsonld_context.as_ref());
    let Ok(compacted) = compact_for_notification(engine, &entity, &context, compaction_cache).await else {
        return vec![];
    };
    let mut compacted = compacted;

    if is_entity_deleted && sub.notification_show_changes {
        fix_null_language_maps(&mut compacted);
    }

    if sub.notification_show_changes
        && let Some(prev) = previous_attributes
    {
        let prev_entity = match prev {
            Value::Array(arr) => match arr.first() {
                Some(value) => value.clone(),
                None => Value::Null,
            },
            other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => other.clone(),
        };
        if let Ok(prev_compacted) = engine
            .compact(
                prev_entity,
                &context,
                CompactOptions {
                    strip_context: ContextStripping::Strip,
                },
            )
            .await
        {
            let prev_compacted = normalize_compacted_entity(prev_compacted);
            let exact_attribute_instance = matches!(
                trigger_reason,
                NotificationTrigger::AttributeCreated | NotificationTrigger::AttributeUpdated | NotificationTrigger::AttributeDeleted
            );
            inject_previous_values(
                &mut compacted,
                &prev_compacted,
                changed_attributes,
                changed_dataset_id,
                exact_attribute_instance,
                engine,
                &context,
            )
            .await;
        }
    }

    let mut all_compacted = vec![compacted];
    if let Some(join) = sub.notification_join {
        if join == JoinMode::Flat
            && let Ok(main_entity) = deserialize_exact::<Entity>(&all_compacted[0])
        {
            match resolve_linked_cached(
                entity_query,
                &main_entity,
                sub.notification_join_level,
                sub.notification_sys_attrs,
                &context,
                compaction_cache,
            )
            .await
            {
                Ok(resolved) => {
                    all_compacted.reserve(resolved.as_values.len());
                    for linked_val in resolved.as_values.iter() {
                        all_compacted.push(linked_val.clone());
                    }
                }
                Err(e) => {
                    debug!("Failed to resolve linked entities for join=flat: {}", e);
                }
            }
        } else if join == JoinMode::Inline
            && let Ok(main_entity) = deserialize_exact::<Entity>(&all_compacted[0])
        {
            match resolve_linked_cached(
                entity_query,
                &main_entity,
                sub.notification_join_level,
                sub.notification_sys_attrs,
                &context,
                compaction_cache,
            )
            .await
            {
                Ok(resolved) => {
                    let payload = take(&mut all_compacted[0]);
                    match SnapshotRenderer::new(payload).with_inline_linked_entities(&resolved.by_id).render() {
                        Ok(rendered) => all_compacted[0] = rendered,
                        Err(error) => {
                            debug!("Failed to apply join=inline to notification payload: {}", error);
                        }
                    }
                }
                Err(e) => {
                    debug!("Failed to resolve linked entities for join=inline: {}", e);
                }
            }
        }
    }

    let format = sub.notification_format.unwrap_or_default();
    let mut result = Vec::with_capacity(all_compacted.len());
    for item in all_compacted {
        let mut formatted = render_format(item, format, sub.notification_join);
        // The same targeted walk the query response uses (clause 4.5.1): only a
        // Relationship's `object` and a ListRelationship's `objectList` are
        // percent-encoded entity ids to decode, so a Property value carrying a
        // `%` is not corrupted, and a notification decodes exactly as a GET of
        // the same entity does.
        decode_percent_encoded_uris(&mut formatted);
        result.push(formatted);
    }

    result
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
