//! Narrowing matched subscriptions by the shared spatial subscription index.
//!
//! The index is a superset filter over `geoQ`: a subscription it does not
//! return for the entity's geometry cannot match, while one it does return is
//! still re-checked by the exact predicate in `matches_entity_filters`.
//!
//! The index is asked once per `GeoProperty` the candidates filter on, and the
//! answer is shared by every candidate filtering on it, so one change costs one
//! probe per `GeoProperty` rather than one per geo subscription.

use crate::{cache::SubscriptionCache, compiled::CompiledSubscription};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_geometry::planar::PlanarGeometry;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_spatial_index::key::SpatialKey;
use iri_rs::IriBuf;
use std::{
    collections::{HashMap, HashSet},
    iter,
};

/// Which subscriptions the spatial index has one entity state in scope of.
///
/// Subscriptions whose `geoQ` restricts by no compiled geoquery are admitted;
/// the exact predicate refuses an unusable one. The entity is probed with
/// every instance of the target `GeoProperty` rather than with its default
/// one: clause 4.10 counts one instance meeting the restriction as enough, so
/// a subscription reached only by a `datasetId`-tagged instance must be
/// admitted. Off the live branch the index holds nothing and every candidate
/// is admitted.
pub struct SpatialReach<'a> {
    /// The cache whose spatial index is probed.
    cache: &'a SubscriptionCache,
    /// The branch the change landed in.
    target: &'a ContextTarget,
    /// Whether the index covers the branch the change landed in.
    indexed: bool,
    /// The entity state probed.
    entity: &'a EntityState,
    /// The types the entity is probed under.
    entity_types: &'a [EntityTypeName],
    /// The subscriptions reached per `GeoProperty` probed so far.
    reached: HashMap<AttributeName, HashSet<Identifier>>,
}

impl<'a> SpatialReach<'a> {
    /// The reach of `entity`, typed `entity_types`, in `target`, probed lazily.
    #[must_use]
    pub fn new(cache: &'a SubscriptionCache, target: &'a ContextTarget, entity: &'a EntityState, entity_types: &'a [EntityTypeName]) -> SpatialReach<'a> {
        SpatialReach {
            cache,
            target,
            indexed: target.permits_federation(),
            entity,
            entity_types,
            reached: HashMap::new(),
        }
    }

    /// Whether the index has the entity in scope of `subscription`'s `geoQ`.
    ///
    /// The index holds the `geoQ` of the versions in force only, so a
    /// superseded selection the index does not reach is admitted, and the
    /// exact predicate decides it.
    pub fn admits(&mut self, subscription: &CompiledSubscription) -> bool {
        let Some(geo_q) = subscription.compiled_geo_q.restriction().filter(|_| self.indexed) else {
            return true;
        };
        if !self.reached.contains_key(&geo_q.geoproperty) {
            let reached = self.probe(&geo_q.geoproperty);
            self.reached.insert(geo_q.geoproperty.clone(), reached); // clone: the map owns its key, which the subscription keeps
        }
        self.reached.get(&geo_q.geoproperty).is_some_and(|ids| ids.contains(&subscription.id)) || !self.cache.is_in_force(self.target, subscription)
    }

    /// Every subscription the index admits for some instance of the entity's
    /// `GeoProperty` `geoproperty`, under any of its types or the wildcard
    /// partition.
    ///
    /// An entity carrying no instance of the property reaches none (clause
    /// 4.10). An untyped or boolean-typed geo subscription is indexed under
    /// the wildcard partition, which no concrete-type key reaches, so that
    /// partition is always probed too.
    fn probe(&self, geoproperty: &AttributeName) -> HashSet<Identifier> {
        let spatial = self.cache.spatial();
        let tenant = self.cache.tenant();
        // clone: every spatial key owns its tenant and property name
        let keys: Vec<SpatialKey> = self
            .entity_types
            .iter()
            .filter_map(|type_name| IriBuf::new(type_name.clone()).ok())
            .map(|type_iri| SpatialKey::new(tenant.clone(), type_iri, geoproperty.clone()))
            .chain(iter::once(SpatialKey::any_type(tenant.clone(), geoproperty.clone())))
            .collect();
        let mut reached = HashSet::new();
        for geometry in self.entity.geo_property_geometries(geoproperty.as_str()).map(PlanarGeometry::project) {
            reached.extend(keys.iter().flat_map(|key| spatial.candidates(key, &geometry)));
        }
        reached
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        cache::SubscriptionCache,
        compiled::CompiledSubscription,
        geo_filter::SubscriptionGeoFilter,
        notifying_since::NotifyingSince,
        spatial_narrowing::SpatialReach,
    };
    use andromeda_context_commit::{store_instant::StoreInstant, subscription_key::SubscriptionKey};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use andromeda_ngsi_ld_model::{
        notification::NotificationTrigger,
        subscription::{SubscriptionStatus, entity_selector::EntitySelector, standing::SubscriptionStanding},
    };
    use andromeda_ngsi_ld_query::{filter::language_matching::LanguageMatching, geo::input::GeoQueryInput, projection::compiled::Projection};
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_tenancy::tenant_ref::TenantRef;
    use chrono::{DateTime, Utc};
    use serde_json::{Value, json};
    use std::{num::NonZeroU32, sync::Arc};
    use url::Url;

    const BUILDING: &str = "https://example.com/Building";
    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";
    const OBSERVATION_POINT: &str = "https://example.com/observationPoint";

    /// Where a subscription's `geoQ` looks for the entity.
    struct Near {
        /// The reference point, longitude then latitude.
        at: [f64; 2],
        /// The `GeoProperty` the `geoQ` names.
        geoproperty: &'static str,
    }

    /// A subscription keyed `key` on Buildings, or on every type when
    /// `typed` is false, restricted by `near;maxDistance==200` around `near`
    /// when given.
    fn subscription(key: i64, typed: bool, near: Option<Near>) -> CompiledSubscription {
        let compiled_geo_q = near.map_or(SubscriptionGeoFilter::Unrestricted, |near| {
            let geo_q: GeoQueryInput = serde_json::from_value(json!({
                "georel": "near;maxDistance==200",
                "geometry": "Point",
                "coordinates": near.at,
                "geoproperty": near.geoproperty
            }))
            .expect("a readable geoQ");
            SubscriptionGeoFilter::from_stored(Some(&geo_q), "urn:ngsi-ld:Subscription:fixture")
        });
        let selectors = if typed {
            vec![EntitySelector {
                id: None,
                id_pattern: None,
                type_: BUILDING.parse().expect("a type selection"),
            }]
        } else {
            Vec::new()
        };
        CompiledSubscription {
            id: format!("urn:ngsi-ld:Subscription:{key}").parse().expect("valid URI"),
            key: SubscriptionKey::try_from(key).expect("a valid key"),
            standing: SubscriptionStanding {
                status: SubscriptionStatus::Active,
                expires_at: None,
            },
            endpoint_uri: Url::parse("http://example.com/notify").expect("valid URL"),
            endpoint_accept: None,
            endpoint_receiver_info: None,
            endpoint_notifier_info: None,
            endpoint_cooldown: None,
            endpoint_timeout: None,
            triggers: vec![NotificationTrigger::EntityCreated],
            watched_attrs: None,
            selectors,
            throttling_secs: None,
            time_interval_secs: None,
            notification_attributes: Vec::new(),
            notification_format: None,
            notification_sys_attrs: SysAttrsInclusion::Strip,
            notification_show_changes: false,
            jsonld_context: None,
            notifying_since: NotifyingSince {
                active: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
                selected: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
            },
            compiled_q: None,
            language_matching: LanguageMatching::TagEquality,
            compiled_geo_q,
            compiled_scope_q: None,
            dataset_id: None,
            notification_join: None,
            notification_join_level: NonZeroU32::MIN,
            compiled_csf: None,
            local_only: false,
            ngsild_conformance: None,
            lang: None,
            notification_projection: Projection::default(),
        }
    }

    /// The live cache holding `subscriptions`, and each of them as it cached it.
    fn cached(subscriptions: Vec<CompiledSubscription>) -> (SubscriptionCache, Vec<Arc<CompiledSubscription>>) {
        let cache = SubscriptionCache::new(TenantRef::UnnamedDefault, LanguageMatching::TagEquality);
        let ids: Vec<_> = subscriptions.iter().map(|subscription| subscription.id.clone()).collect();
        for subscription in subscriptions {
            cache.insert(&ContextTarget::Live, subscription);
        }
        let cached = ids
            .iter()
            .map(|id| cache.get(&ContextTarget::Live, id).expect("the subscription is cached"))
            .collect();
        (cache, cached)
    }

    /// One `GeoProperty` instance at `at`, tagged `dataset_id` when given.
    fn point(at: [f64; 2], dataset_id: Option<&str>) -> Value {
        let mut instance = json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{
                "@type": ["https://purl.org/geojson/vocab#Point"],
                "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": at[0]}, {"@value": at[1]}]}]
            }]
        });
        if let (Some(dataset_id), Some(members)) = (dataset_id, instance.as_object_mut()) {
            members.insert("https://uri.etsi.org/ngsi-ld/datasetId".to_owned(), json!([{"@id": dataset_id}]));
        }
        instance
    }

    /// A Building carrying the `GeoProperty` instances given under each name.
    fn building(geo_properties: &[(&str, Vec<Value>)]) -> EntityState {
        let mut node = json!({"@id": "urn:ngsi-ld:Building:1", "@type": [BUILDING]});
        if let Some(members) = node.as_object_mut() {
            for (name, instances) in geo_properties {
                members.insert((*name).to_owned(), Value::Array(instances.clone()));
            }
        }
        EntityState::from_slice(&serde_json::to_vec(&json!([node])).expect("JSON")).expect("a stored entity decodes")
    }

    fn building_type() -> Vec<EntityTypeName> {
        vec![BUILDING.parse().expect("a type name")]
    }

    /// 0.01 degrees of longitude at latitude 52.52 is about 677 m, beyond the
    /// 200 m every fixture `geoQ` allows (clause 4.10 `near;maxDistance`).
    const HERE: [f64; 2] = [13.4, 52.52];
    const AWAY: [f64; 2] = [13.41, 52.52];

    #[test]
    fn a_near_subscription_is_admitted_only_for_an_entity_within_its_distance() {
        let (cache, subscriptions) = cached(vec![
            subscription(
                1,
                true,
                Some(Near {
                    at: HERE,
                    geoproperty: LOCATION,
                }),
            ),
            subscription(
                2,
                true,
                Some(Near {
                    at: AWAY,
                    geoproperty: LOCATION,
                }),
            ),
        ]);
        let entity = building(&[(LOCATION, vec![point([13.4001, 52.52], None)])]);
        let types = building_type();
        let live = ContextTarget::Live;
        let mut reach = SpatialReach::new(&cache, &live, &entity, &types);
        let admitted: Vec<bool> = subscriptions.iter().map(|subscription| reach.admits(subscription)).collect();
        pretty_assertions::assert_eq!(admitted, vec![true, false]);
    }

    #[test]
    fn a_subscription_without_a_geo_q_is_admitted_without_probing_the_index() {
        let (cache, subscriptions) = cached(vec![subscription(1, true, None)]);
        let entity = building(&[]);
        let types = building_type();
        let live = ContextTarget::Live;
        let mut reach = SpatialReach::new(&cache, &live, &entity, &types);
        assert!(subscriptions.iter().all(|subscription| reach.admits(subscription)));
        assert!(reach.reached.is_empty(), "no GeoProperty is probed for an unrestricted subscription");
    }

    #[test]
    fn subscriptions_on_one_geo_property_are_decided_by_one_probe_of_it() {
        let (cache, subscriptions) = cached(vec![
            subscription(
                1,
                true,
                Some(Near {
                    at: HERE,
                    geoproperty: LOCATION,
                }),
            ),
            subscription(
                2,
                true,
                Some(Near {
                    at: AWAY,
                    geoproperty: LOCATION,
                }),
            ),
            subscription(
                3,
                true,
                Some(Near {
                    at: HERE,
                    geoproperty: LOCATION,
                }),
            ),
            subscription(
                4,
                true,
                Some(Near {
                    at: HERE,
                    geoproperty: OBSERVATION_POINT,
                }),
            ),
        ]);
        let entity = building(&[(LOCATION, vec![point(HERE, None)]), (OBSERVATION_POINT, vec![point(AWAY, None)])]);
        let types = building_type();
        let live = ContextTarget::Live;
        let mut reach = SpatialReach::new(&cache, &live, &entity, &types);
        let admitted: Vec<bool> = subscriptions.iter().map(|subscription| reach.admits(subscription)).collect();
        pretty_assertions::assert_eq!(admitted, vec![true, false, true, false]);
        pretty_assertions::assert_eq!(reach.reached.len(), 2, "one probe per GeoProperty named");
    }

    /// Clause 4.10: an Entity without the `GeoProperty` the geoquery names
    /// does not meet the restriction.
    #[test]
    fn an_entity_without_the_named_geo_property_reaches_no_subscription_on_it() {
        let (cache, subscriptions) = cached(vec![subscription(
            1,
            true,
            Some(Near {
                at: HERE,
                geoproperty: OBSERVATION_POINT,
            }),
        )]);
        let entity = building(&[(LOCATION, vec![point(HERE, None)])]);
        let types = building_type();
        let live = ContextTarget::Live;
        let mut reach = SpatialReach::new(&cache, &live, &entity, &types);
        assert!(!subscriptions.iter().any(|subscription| reach.admits(subscription)));
    }

    /// Clause 5.8.6: with several instances of the `GeoProperty`, it is
    /// sufficient that any one of them meets the restriction.
    #[test]
    fn a_subscription_reached_only_by_a_dataset_instance_is_admitted() {
        let (cache, subscriptions) = cached(vec![subscription(
            1,
            true,
            Some(Near {
                at: HERE,
                geoproperty: LOCATION,
            }),
        )]);
        let entity = building(&[(LOCATION, vec![point(AWAY, None), point(HERE, Some("urn:ngsi-ld:Dataset:a"))])]);
        let types = building_type();
        let live = ContextTarget::Live;
        let mut reach = SpatialReach::new(&cache, &live, &entity, &types);
        assert!(subscriptions.iter().all(|subscription| reach.admits(subscription)));
    }

    #[test]
    fn a_geo_subscription_without_a_type_selector_is_reached_through_the_wildcard_partition() {
        let (cache, subscriptions) = cached(vec![
            subscription(
                1,
                false,
                Some(Near {
                    at: HERE,
                    geoproperty: LOCATION,
                }),
            ),
            subscription(
                2,
                false,
                Some(Near {
                    at: AWAY,
                    geoproperty: LOCATION,
                }),
            ),
        ]);
        let entity = building(&[(LOCATION, vec![point(HERE, None)])]);
        let types = building_type();
        let live = ContextTarget::Live;
        let mut reach = SpatialReach::new(&cache, &live, &entity, &types);
        let admitted: Vec<bool> = subscriptions.iter().map(|subscription| reach.admits(subscription)).collect();
        pretty_assertions::assert_eq!(admitted, vec![true, false]);
    }

    /// The index holds the `geoQ` in force, so a superseded selection near
    /// the entity is admitted although the index reaches only the newer
    /// `geoQ`, and the exact predicate decides it.
    #[test]
    fn a_superseded_selection_the_index_does_not_reach_is_admitted() {
        let near = |at| Some(Near { at, geoproperty: LOCATION });
        let (cache, superseded) = cached(vec![subscription(1, true, near(HERE))]);
        let mut moved = subscription(1, true, near(AWAY));
        moved.notifying_since.selected = StoreInstant::new(DateTime::<Utc>::UNIX_EPOCH);
        cache.insert(&ContextTarget::Live, moved);
        let in_force = cache.get(&ContextTarget::Live, &superseded[0].id).expect("the subscription is cached");
        let entity = building(&[(LOCATION, vec![point(HERE, None)])]);
        let types = building_type();
        let live = ContextTarget::Live;
        let mut reach = SpatialReach::new(&cache, &live, &entity, &types);
        assert!(reach.admits(&superseded[0]));
        assert!(!reach.admits(&in_force), "the selection in force is still narrowed by the index");
    }
}
