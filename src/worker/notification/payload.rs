//! Notification payload building: showChanges, previous values, datasetId
//! filtering, nullification, and format simplification.

use crate::{
    service::{
        compiled_subscription::CompiledSubscription,
        entity::{
            query::EntityQueryService,
            sys_attrs::{expanded_entity_object_mut, inject_attr_sys_attrs},
        },
    },
    worker::notification::payload::{
        cache::{NotificationCompactionCache, compact_entity_with_cache, resolve_linked_cached},
        context::{context_for_subscription, normalize_compacted_entity, strip_to_structural_keys},
        deletion::{fix_null_language_maps, inject_deleted_attributes, nullify_entity_attributes},
        filtering::{filter_entity_attributes, filter_entity_by_dataset_id, inject_previous_values},
    },
};
use andromeda_delivery::request::notification_media_type;
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{
        options::{CompactOptions, ContextStripping},
        processor::JsonLdEngine,
    },
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::{entity::Entity, notification::NotificationTrigger};
use andromeda_ngsi_ld_render::{simplify::simplify_entity, snapshot::SnapshotRenderer, uri_decoding::decode_percent_encoded_uris};
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, response_format::ResponseFormat, sys_attrs::SysAttrsInclusion};
use andromeda_ngsi_ld_vocabulary::{expanded::properties as expanded_properties, prefix};
use chrono::{DateTime, Utc};
use serde_json::Value;
use tracing::debug;

pub(super) struct NotificationBuildParams<'a> {
    pub(super) engine: &'a JsonLdEngine,
    pub(super) entity_data: &'a Value,
    pub(super) previous_attributes: Option<&'a Value>,
    pub(super) sub: &'a CompiledSubscription,
    pub(super) trigger_reason: &'a NotificationTrigger,
    pub(super) changed_attributes: &'a [AttributeName],
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
        occurred_at,
        entity_query,
        compaction_cache,
    } = params;

    let mut entity = match entity_data {
        Value::Array(arr) => match arr.first() {
            Some(value) => value.clone(),
            None => Value::Null,
        },
        other => other.clone(),
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
        inject_deleted_attributes(&mut entity, effective_changed, previous_attributes, deleted_at);

        if sub.notification_sys_attrs == SysAttrsInclusion::Include
            && let Some(obj) = expanded_entity_object_mut(&mut entity)
        {
            let ca = obj.get(expanded_properties::CREATED_AT.as_str()).cloned();
            let ma = obj.get(expanded_properties::MODIFIED_AT.as_str()).cloned();
            if let (Some(ca), Some(ma)) = (ca, ma) {
                let default_context_prefix = format!("{}default-context/", prefix::NGSI_LD.as_str());
                inject_attr_sys_attrs(obj, &ca, &ma, &default_context_prefix);
            }
        }
    }

    if let Some(ref dataset_ids) = sub.dataset_id {
        filter_entity_by_dataset_id(&mut entity, dataset_ids);
    }

    let entity = if let Some(ref attrs) = sub.notification_attributes {
        filter_entity_attributes(&entity, attrs)
    } else {
        entity
    };

    let context = context_for_subscription(sub);
    // The same reading the transport makes: a body carries its own `@context`
    // only when the notification goes out as `application/ld+json`.
    let include_context = notification_media_type(sub.endpoint_accept.as_ref()).include_context_in_body();

    let compacted = match compact_entity_with_cache(
        engine,
        &entity,
        &context,
        if include_context { ContextStripping::Keep } else { ContextStripping::Strip },
        compaction_cache,
    )
    .await
    {
        Ok(value) => value,
        Err(e) => {
            debug!("Failed to compact notification entity with subscription context: {}", e);
            match compact_entity_with_cache(
                engine,
                &entity,
                &ContextSpec::core_only(),
                if include_context { ContextStripping::Keep } else { ContextStripping::Strip },
                compaction_cache,
            )
            .await
            {
                Ok(value) => value,
                Err(e) => {
                    debug!("Failed to compact notification entity with core context: {}", e);
                    return vec![];
                }
            }
        }
    };

    let compacted = if include_context {
        if let Some(ref ctx_url) = sub.jsonld_context {
            let mut c = compacted;
            if let Some(obj) = c.as_object_mut() {
                obj.insert("@context".to_string(), Value::String(ctx_url.as_str().to_owned()));
            }
            c
        } else {
            compacted
        }
    } else {
        compacted
    };

    let mut compacted = normalize_compacted_entity(compacted);

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
            other => other.clone(),
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
            inject_previous_values(&mut compacted, &prev_compacted, changed_attributes, engine, &context).await;
        }
    }

    let mut all_compacted = vec![compacted];
    if let Some(join) = sub.notification_join {
        if join == JoinMode::Flat
            && let Ok(main_entity) = serde_json::from_value::<Entity>(all_compacted[0].clone())
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
            && let Ok(main_entity) = serde_json::from_value::<Entity>(all_compacted[0].clone())
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
                    let payload = std::mem::take(&mut all_compacted[0]);
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

    let is_simplified = sub.notification_format.is_some_and(ResponseFormat::is_simplified);
    let has_inline_join = sub.notification_join == Some(JoinMode::Inline);
    let mut result = Vec::with_capacity(all_compacted.len());
    for item in all_compacted {
        let formatted = if is_simplified {
            if has_inline_join {
                match SnapshotRenderer::new(item.clone()).with_mode(ResponseFormat::Simplified).render() {
                    Ok(simplified) => simplified,
                    Err(error) => {
                        debug!("Failed to simplify inline-joined notification payload: {}", error);
                        item
                    }
                }
            } else {
                match serde_json::from_value::<Entity>(item.clone()) {
                    Ok(entity) => simplify_entity(&entity),
                    Err(e) => {
                        debug!("Failed to simplify entity for keyValues: {}", e);
                        item
                    }
                }
            }
        } else {
            item
        };
        let mut formatted = formatted;
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
