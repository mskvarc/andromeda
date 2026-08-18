//! Narrowing matched subscriptions by the shared spatial subscription index.
//!
//! The index is a superset filter over `geoQ`: a subscription it does not
//! return for the entity's geometry cannot match, while one it does return is
//! still re-checked by the exact predicate in `matches_entity_filters`.

use crate::{cache::SubscriptionCache, compiled::CompiledSubscription};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_model::expanded::geo_property::geo_property_instance_geometries;
use andromeda_spatial_index::key::SpatialKey;
use iri_rs::IriBuf;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};

/// Narrow `matching` to subscriptions whose pre-compiled `geoQ` actually has
/// the entity in scope, by consulting the cache's spatial index.
///
/// Subscriptions without a `compiled_geo_q` are kept verbatim. The entity is
/// probed with every instance of the target GeoProperty rather than with its
/// default one: clause 4.10 counts one instance meeting the restriction as
/// enough, so a subscription reached only by a `datasetId`-tagged instance
/// must survive the narrowing.
pub fn narrow_by_spatial_index(
    cache: &SubscriptionCache,
    matching: Vec<Arc<CompiledSubscription>>,
    entity_data: &Value,
    entity_types: &[EntityTypeName],
) -> Vec<Arc<CompiledSubscription>> {
    let spatial = cache.spatial();
    let tenant = cache.tenant();

    // One read of each distinct geoproperty, cached across the subscriptions
    // that target it. A property the entity does not carry is cached as an
    // empty instance list too, so a miss is not re-read either.
    let mut geometries: HashMap<AttributeName, Vec<Geometry>> = HashMap::new();
    let mut kept: Vec<Arc<CompiledSubscription>> = Vec::new();

    for sub in matching {
        let Some(ref geo_q) = sub.compiled_geo_q else {
            kept.push(sub);
            continue;
        };

        let geoproperty = &geo_q.geoproperty;
        if !geometries.contains_key(geoproperty) {
            let instances: Vec<Geometry> = geo_property_instance_geometries(entity_data, geoproperty.as_str()).collect();
            // clone: the cache owns its key, which the subscription keeps
            geometries.insert(geoproperty.clone(), instances);
        }
        // The entity carries no instance of this geoproperty, so no
        // subscription filtering on it can match (clause 4.10).
        let instances = geometries.get(geoproperty).map_or(&[][..], Vec::as_slice);
        if instances.is_empty() {
            continue;
        }

        let reached = instances.iter().any(|entity_geometry| {
            let typed_hit = entity_types.iter().filter_map(|type_name| IriBuf::new(type_name.clone()).ok()).any(|type_iri| {
                // clone: every spatial key owns its tenant and property name
                let key = SpatialKey::new(tenant.clone(), type_iri, geoproperty.clone());
                spatial.candidates(&key, entity_geometry).iter().any(|candidate| candidate == &sub.id)
            });
            // An untyped or boolean-typed geo subscription is indexed under the
            // wildcard partition, which no concrete-type key reaches; query it
            // too, or such a subscription is never evaluated and silently
            // dropped (clause 4.10 still applies to it).
            typed_hit || {
                let key = SpatialKey::any_type(tenant.clone(), geoproperty.clone()); // clone: the key owns both
                spatial.candidates(&key, entity_geometry).iter().any(|candidate| candidate == &sub.id)
            }
        });
        if reached {
            kept.push(sub);
        }
    }

    kept
}
