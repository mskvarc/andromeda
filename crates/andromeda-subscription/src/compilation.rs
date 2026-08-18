//! Compilation of persisted subscriptions into matching state.

use crate::{
    compiled::{CompiledCsrSubscription, CompiledSubscription},
    stored_subscription::StoredSubscription,
};
use andromeda_ngsi_ld_geometry::{geometry::Geometry, geometry_type::GeometryType};
use andromeda_ngsi_ld_model::{
    notification::effective_notification_triggers,
    subscription::{EntitySelector, Subscription},
};
use andromeda_ngsi_ld_query::geo::query::GeoQuery;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use std::{collections::HashSet, num::NonZeroU32};
use tracing::debug;

/// Compile a stored subscription into a pre-compiled form for fast matching.
///
/// Takes the stored pair rather than the resource alone: matching ends in a
/// delivery enqueued against the surrogate key, so the key has to survive
/// compilation alongside everything the match itself needs.
pub fn compile_subscription(stored: &StoredSubscription) -> CompiledSubscription {
    let sub = &stored.subscription;
    let selectors: Vec<EntitySelector> = sub.selection.entities.clone().unwrap_or_default();
    let triggers = effective_notification_triggers(sub.selection.notification_trigger.as_deref());
    let watched_attrs = sub.selection.watched_attributes.as_ref().cloned().map(HashSet::from_iter);
    CompiledSubscription {
        id: sub.id.clone(), // clone: compiled state outlives the persisted value.
        key: stored.key,
        status: sub.lifecycle.status,
        endpoint_uri: sub.notification.endpoint.uri.clone(), // clone: compiled state owns endpoint configuration.
        endpoint_accept: sub.notification.endpoint.accept.clone(), // clone: compiled state owns endpoint configuration.
        endpoint_receiver_info: sub.notification.endpoint.receiver_info.clone(), // clone: compiled state owns endpoint configuration.
        endpoint_notifier_info: sub.notification.endpoint.notifier_info.clone(), // clone: compiled state owns endpoint configuration.
        triggers,
        watched_attrs,
        selectors,
        throttling_secs: sub.delivery.throttling,
        time_interval_secs: sub.delivery.time_interval,
        notification_attributes: sub.notification.attributes.clone(), // clone: compiled state owns its projection.
        notification_format: sub.notification.format,
        notification_sys_attrs: if sub.notification.sys_attrs.unwrap_or(false) {
            SysAttrsInclusion::Include
        } else {
            SysAttrsInclusion::Strip
        },
        notification_show_changes: sub.notification.show_changes.unwrap_or(false),
        jsonld_context: sub.delivery.jsonld_context.clone(), // clone: compiled state owns the context URL.
        expires_at: sub.lifecycle.expires_at,
        created_at: sub.lifecycle.created_at,
        compiled_q: sub.filters.q.clone(), // clone: compiled state owns its filter AST.
        compiled_geo_q: sub.filters.geo_q.as_ref().and_then(|value| compile_geo_q(value, sub.id.as_ref())),
        compiled_scope_q: sub.filters.scope_q.clone(), // clone: compiled state owns its scope AST.
        dataset_id: sub.filters.dataset_id.clone(),    // clone: compiled state owns dataset identifiers.
        notification_join: sub.notification.join,
        // Clause 4.5.23: `joinLevel` defaults to one hop and is otherwise
        // honoured exactly. Zero never reaches here -- the model rejects it at
        // deserialization.
        notification_join_level: sub.notification.join_level.unwrap_or(NonZeroU32::MIN),
        compiled_csf: sub.filters.csf.clone(), // clone: compiled state owns its filter AST.
        local_only: sub.delivery.local_only.unwrap_or(false),
    }
}

/// Compile a context-source subscription for in-memory registration matching.
pub fn compile_csource_subscription(sub: &Subscription) -> CompiledCsrSubscription {
    let selectors: Vec<EntitySelector> = sub.selection.entities.clone().unwrap_or_default();
    let watched_attrs = sub.selection.watched_attributes.as_ref().cloned().map(HashSet::from_iter);
    CompiledCsrSubscription {
        id: sub.id.clone(), // clone: compiled state outlives the persisted value.
        status: sub.lifecycle.status,
        endpoint_uri: sub.notification.endpoint.uri.clone(), // clone: compiled state owns endpoint configuration.
        endpoint_accept: sub.notification.endpoint.accept.clone(), // clone: compiled state owns endpoint configuration.
        endpoint_receiver_info: sub.notification.endpoint.receiver_info.clone(), // clone: compiled state owns endpoint configuration.
        endpoint_notifier_info: sub.notification.endpoint.notifier_info.clone(), // clone: compiled state owns endpoint configuration.
        selectors,
        watched_attrs,
        throttling_secs: sub.delivery.throttling,
        time_interval_secs: sub.delivery.time_interval,
        jsonld_context: sub.delivery.jsonld_context.clone(), // clone: compiled state owns the context URL.
        expires_at: sub.lifecycle.expires_at,
        compiled_csf: sub.filters.csf.clone(), // clone: compiled state owns its filter AST.
    }
}

/// Pre-parse a stored `geoQ` into the request-side [`GeoQuery`] type (clause 4.10).
pub fn compile_geo_q(geo_q: &serde_json::Value, sub_id: &str) -> Option<GeoQuery> {
    let geo_obj = geo_q.as_object()?;
    let georel_str = geo_obj.get("georel").and_then(serde_json::Value::as_str)?;
    let geometry_str = geo_obj.get("geometry").and_then(serde_json::Value::as_str)?;
    let coordinates = match geo_obj.get("coordinates")? {
        serde_json::Value::String(raw) => serde_json::from_str::<serde_json::Value>(raw).ok()?,
        other @ (serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::Array(_)
        | serde_json::Value::Object(_)) => other.clone(), // clone: compiled state owns its reference geometry.
    };
    let raw_geoproperty = geo_obj
        .get("geoproperty")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(expanded_properties::LOCATION.as_str());
    let geoproperty = match raw_geoproperty.parse() {
        Ok(geoproperty) => geoproperty,
        Err(error) => {
            debug!(subscription = sub_id, %error, "Invalid subscription geoQ geoproperty");
            return None;
        }
    };
    let geometry_type = match geometry_str.parse::<GeometryType>() {
        Ok(geometry_type) => geometry_type,
        Err(error) => {
            debug!(subscription = sub_id, %error, "Invalid subscription geoQ geometry type");
            return None;
        }
    };
    let geometry = match Geometry::from_type_and_coordinates(geometry_type, coordinates) {
        Ok(geometry) => geometry,
        Err(error) => {
            debug!(subscription = sub_id, %error, "Invalid subscription geoQ geometry");
            return None;
        }
    };
    Some(GeoQuery {
        georel: georel_str.parse().ok()?,
        geometry,
        geoproperty,
    })
}

#[cfg(test)]
mod tests {
    use crate::compilation::compile_geo_q;
    use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;

    #[test]
    fn malformed_geo_query_does_not_compile() {
        assert_eq!(compile_geo_q(&serde_json::json!({}), "urn:ngsi-ld:Subscription:test"), None);
    }

    #[test]
    fn absent_geoproperty_uses_the_ngsi_ld_location_iri() {
        let compiled = compile_geo_q(
            &serde_json::json!({"georel": "within", "geometry": "Point", "coordinates": [13.4, 52.5]}),
            "urn:ngsi-ld:Subscription:test",
        );
        assert_eq!(
            compiled.as_ref().map(|query| query.geoproperty.as_str()),
            Some(expanded_properties::LOCATION.as_str())
        );
    }
}
