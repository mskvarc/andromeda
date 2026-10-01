//! Proof that declining a predicate does not change the answer.
//!
//! The residual machinery exists so a store that cannot push a predicate down
//! returns the same entities as one that can. With a single adapter whose every
//! axis is at its full value, nothing in production ever takes the deferred
//! branch, so that equality is asserted here instead — against an in-memory
//! store whose [`StorageProfile`] is a test parameter.
//!
//! The store applies the predicates it kept using a reference reading of the
//! specification written in this module, deliberately independent of
//! [`residual_filter::matches`]. When the two disagree, the profile that declines
//! the predicate returns a different set from the profile that pushes it down,
//! and the comparison fails. That is the only construction available without a
//! second backend that makes the residual and the pushdown check each other.
//!
//! [`StorageProfile`]: crate::storage_profile::StorageProfile

#[cfg(test)]
mod tests {
    use crate::{
        config::EntityConfig,
        entity_snapshot::EntitySnapshot,
        error::EntityError,
        pagination_outcome::PaginationOutcome,
        port::{query::EntityQueryStore, reader::EntityReader},
        query::EntityQueryService,
        query_outcome::QueryOutcome,
        spatial_acceleration::SpatialAcceleration,
        storage_profile::{GeoPredicatePushdown, ScopePushdown, StorageProfile, ValueFilterPushdown},
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_jsonld::engine::processor::JsonLdEngine;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, positive_distance::PositiveDistance, relation::matches};
    use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
    use andromeda_ngsi_ld_model::{
        expanded::{
            entity_member::{extract_id, extract_types},
            geo_property::geo_property_instance_geometries,
            scope_member::extract_scopes,
        },
        notification::SysAttrsSnapshot,
    };
    use andromeda_ngsi_ld_query::{
        entity::compiled::CompiledEntityQuery,
        entity_selection::{ConsideredEntities, EntitySelection},
        filter::{eval, parser},
        geo::query::GeoQuery,
        pagination::{CountPolicy, Pagination},
        scope::query::ScopeQuery,
    };
    use andromeda_tenancy::tenant_ref::TenantRef;
    use async_trait::async_trait;
    use serde_json::{Value, json};
    use std::{collections::HashMap, str::FromStr, sync::Arc};

    const TEMPERATURE: &str = "https://example.com/temperature";
    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";
    const BUILDING: &str = "https://example.com/Building";

    /// The reference reading of the predicates this harness drives.
    ///
    /// Written from ETSI GS CIM 009 v1.9.1 rather than from
    /// `ResidualFilter`, so the two are independent witnesses of the same
    /// clauses: clause 4.9 for `q`, clause 4.10 for `georel`, clause 4.18 for
    /// `scopeQ`.
    fn reference_matches(selection: &EntitySelection, entity: &Value) -> bool {
        let q_holds = selection.q_expr.as_ref().is_none_or(|expr| eval::evaluate(expr, entity));
        let geo_holds = selection.geo.as_ref().is_none_or(|geo| {
            // Clause 4.10: one instance of the target GeoProperty meeting the
            // restriction is enough.
            geo_property_instance_geometries(entity, geo.geoproperty.as_str()).any(|subject| matches(&geo.georel, &subject, &geo.geometry))
        });
        let scope_holds = selection.scope_q.as_ref().is_none_or(|scope_q| scope_q.matches(&extract_scopes(entity)));
        // Clause 5.5.14: an EntityMap "fixes the Entities to be considered", so
        // a request left considering none admits no entity at all.
        let considered_holds = match selection.considered {
            ConsideredEntities::Selected => true,
            ConsideredEntities::NoEntity => false,
        };
        // Clause 5.2.33 union of selectors: an unconstrained union admits every
        // entity, otherwise any one selector's id and type must both hold.
        let selectors_hold = !selection.selectors_constrain()
            || selection.selectors.iter().any(|selector| {
                let type_ok = selector.type_expr.as_ref().is_none_or(|type_expr| {
                    let types = extract_types(entity).expect("the fixtures carry readable types");
                    type_expr.matches(&types.iter().map(EntityTypeName::as_str).collect::<Vec<&str>>())
                });
                let id_ok = selector.ids.is_empty() || extract_id(entity).is_ok_and(|id| selector.ids.contains(&id));
                type_ok && id_ok
            });
        q_holds && geo_holds && scope_holds && selectors_hold && considered_holds
    }

    /// An entity store that holds its rows in memory and answers exactly as much
    /// of a query as its profile allows.
    ///
    /// The planning here is the same shape `src/repository/entity/pushdown.rs`
    /// uses, expressed against the public policy API: ask the profile what it
    /// cannot apply, strip that from the query it runs, and hand it back. A
    /// store that leaves a residual takes no window either, because a window over
    /// rows a later filter will thin is wrong (clause 5.5.9).
    struct ProfiledEntityStore {
        entities: Vec<Value>,
        profile: StorageProfile,
    }

    impl ProfiledEntityStore {
        fn new(profile: StorageProfile) -> ProfiledEntityStore {
            ProfiledEntityStore { entities: fixtures(), profile }
        }

        /// The identifiers this store's own predicates admit, and what it could
        /// not apply.
        fn run(&self, compiled: &CompiledEntityQuery) -> QueryOutcome<Identifier> {
            let unapplied = self.profile.unapplied(&compiled.selection);
            let mut pushed = compiled.selection.clone(); // clone: the store runs a narrowed copy of the caller's query
            unapplied.strip_from(&mut pushed);

            let admitted = self
                .entities
                .iter()
                .filter(|entity| reference_matches(&pushed, entity))
                .map(|entity| extract_id(entity).expect("the fixtures carry readable identifiers"));

            if unapplied.is_empty() {
                let all: Vec<Identifier> = admitted.collect();
                let total = match compiled.pagination.count {
                    CountPolicy::Emit => Some(i64::try_from(all.len()).unwrap_or(i64::MAX)),
                    CountPolicy::Omit => None,
                };
                let rows = all
                    .into_iter()
                    .skip(compiled.pagination.offset as usize)
                    .take(compiled.pagination.limit as usize)
                    .collect();
                QueryOutcome {
                    rows,
                    unapplied,
                    pagination: PaginationOutcome::Applied { total },
                }
            } else {
                QueryOutcome {
                    rows: admitted.collect(),
                    unapplied,
                    pagination: PaginationOutcome::Deferred,
                }
            }
        }
    }

    /// The methods this harness does not drive.
    fn unused(method: &str) -> EntityError {
        panic!("ProfiledEntityStore::{method} is not exercised by the residual conformance harness")
    }

    #[async_trait]
    impl EntityReader for ProfiledEntityStore {
        async fn get(&self, _target: &ContextTarget, id: &Identifier) -> Result<Option<Value>, EntityError> {
            // clone: the double hands out owned payloads, as a real store does
            Ok(self.entities.iter().find(|entity| extract_id(entity).as_ref() == Ok(id)).cloned())
        }

        async fn get_batch(&self, _target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, Value>, EntityError> {
            Ok(self
                .entities
                .iter()
                .map(|entity| (extract_id(entity).expect("the fixtures carry readable identifiers"), entity))
                .filter(|(id, _)| ids.contains(id))
                .map(|(id, entity)| (id, entity.clone())) // clone: as above
                .collect())
        }

        async fn get_snapshot(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<EntitySnapshot>, EntityError> {
            Err(unused("get_snapshot"))
        }

        async fn get_snapshot_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, EntitySnapshot>, EntityError> {
            Err(unused("get_snapshot_batch"))
        }

        async fn get_sys_attrs(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<SysAttrsSnapshot>, EntityError> {
            Err(unused("get_sys_attrs"))
        }

        async fn get_sys_attrs_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, SysAttrsSnapshot>, EntityError> {
            Err(unused("get_sys_attrs_batch"))
        }
    }

    #[async_trait]
    impl EntityQueryStore for ProfiledEntityStore {
        fn profile(&self) -> &StorageProfile {
            &self.profile
        }

        async fn query_compiled(&self, _target: &ContextTarget, _compiled: &CompiledEntityQuery) -> Result<QueryOutcome<Value>, EntityError> {
            Err(unused("query_compiled"))
        }

        async fn query_ids(&self, _target: &ContextTarget, compiled: &CompiledEntityQuery) -> Result<QueryOutcome<Identifier>, EntityError> {
            Ok(self.run(compiled))
        }
    }

    fn live_target() -> ContextTarget {
        ContextTarget::Live
    }

    /// One building, at a temperature, at a position, in a scope.
    fn building(index: u32, temperature: f64, longitude: f64, latitude: f64, scope: &str) -> Value {
        json!([{
            "@id": format!("urn:ngsi-ld:Building:{index:03}"),
            "@type": [BUILDING],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": scope}],
            TEMPERATURE: [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": temperature}]}],
            LOCATION: [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": {"type": "Point", "coordinates": [longitude, latitude]}}]}]
        }])
    }

    /// Six buildings spanning both sides of every predicate the harness drives.
    fn fixtures() -> Vec<Value> {
        vec![
            building(1, 10.0, 13.400, 52.500, "/Berlin/Mitte"),
            building(2, 20.0, 13.401, 52.500, "/Berlin/Kreuzberg"),
            building(3, 30.0, 13.402, 52.500, "/Berlin"),
            building(4, 40.0, 2.352, 48.856, "/Paris/Marais"),
            building(5, 50.0, 2.353, 48.856, "/Paris"),
            building(6, 60.0, 13.403, 52.500, "/Berlin/Wedding"),
        ]
    }

    fn near_berlin() -> GeoQuery {
        GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(PositiveDistance::from_str("1000").expect("positive distance")),
                min_distance: None,
            },
            geometry: Geometry::from_geojson_value(&json!({"type": "Point", "coordinates": [13.4, 52.5]})).expect("valid geometry"),
            geoproperty: LOCATION.parse().expect("valid attribute name"),
        }
    }

    /// A query carrying every predicate the three declared axes cover.
    fn query(limit: u32, offset: u32) -> CompiledEntityQuery {
        CompiledEntityQuery::builder()
            .selection(
                EntitySelection::builder()
                    .q_expr(Some(parser::parse("\"https://example.com/temperature\">15").expect("valid q filter")))
                    .geo(Some(near_berlin()))
                    .scope_q(Some(ScopeQuery::parse("/Berlin/#").expect("valid scopeQ")))
                    .build(),
            )
            .pagination(Pagination {
                limit,
                offset,
                count: CountPolicy::Omit,
            })
            .build()
    }

    fn service(profile: StorageProfile) -> EntityQueryService {
        let store = Arc::new(ProfiledEntityStore::new(profile));
        EntityQueryService::new(
            Arc::clone(&store) as Arc<dyn EntityReader>, // clone: the test store supplies both focused capabilities.
            store as Arc<dyn EntityQueryStore>,
            JsonLdEngine::builder().allow_remote(false).build().expect("build engine"),
            SpatialAcceleration::StoreVerified,
            TenantRef::UnnamedDefault,
            None,
            EntityConfig::default().geometry_census_limits(),
        )
    }

    /// Every profile whose axes all narrow, so no query is refused outright.
    ///
    /// The full profile is first: it is the answer every other profile must
    /// reproduce.
    fn narrowing_profiles() -> Vec<(&'static str, StorageProfile)> {
        let mut profiles = Vec::new();
        for (geo_name, geo) in [("full", GeoPredicatePushdown::Full), ("bbox", GeoPredicatePushdown::BoundingBoxOnly)] {
            for (value_name, value_filter) in [("jsonpath", ValueFilterPushdown::JsonPath), ("structured", ValueFilterPushdown::Structured)] {
                for (scope_name, scope) in [("ltree", ScopePushdown::LabelTree), ("prefix", ScopePushdown::PrefixOnly)] {
                    profiles.push((
                        match (geo_name, value_name, scope_name) {
                            ("full", "jsonpath", "ltree") => "geo=full value=jsonpath scope=ltree",
                            ("full", "jsonpath", _) => "geo=full value=jsonpath scope=prefix",
                            ("full", _, "ltree") => "geo=full value=structured scope=ltree",
                            ("full", _, _) => "geo=full value=structured scope=prefix",
                            (_, "jsonpath", "ltree") => "geo=bbox value=jsonpath scope=ltree",
                            (_, "jsonpath", _) => "geo=bbox value=jsonpath scope=prefix",
                            (_, _, "ltree") => "geo=bbox value=structured scope=ltree",
                            (_, _, _) => "geo=bbox value=structured scope=prefix",
                        },
                        StorageProfile {
                            geo_predicates: geo,
                            value_filter,
                            scope,
                        },
                    ));
                }
            }
        }
        profiles
    }

    #[tokio::test]
    async fn every_partial_profile_returns_what_the_full_pushdown_returns() {
        // The point of the whole mechanism: which predicates a store declined is
        // an implementation detail of that store, never a difference the client
        // can observe (clause 5.7.2).
        let expected = service(StorageProfile::POSTGIS_LTREE)
            .query_ids(&live_target(), &query(100, 0))
            .await
            .expect("the full profile answers");
        assert!(!expected.is_empty(), "the fixtures must exercise the predicates");

        for (name, profile) in narrowing_profiles() {
            let actual = service(profile)
                .query_ids(&live_target(), &query(100, 0))
                .await
                .expect("a narrowing profile answers");
            pretty_assertions::assert_eq!(actual, expected, "profile {name} disagreed with the full pushdown");
        }
    }

    #[tokio::test]
    async fn a_deferred_window_lands_on_the_same_page_the_store_would_have_returned() {
        // Clause 5.5.9: the window is over matching entities, so it must be
        // taken after the residual rather than before it.
        for (limit, offset) in [(1, 0), (1, 1), (2, 0), (2, 1), (10, 0), (10, 5)] {
            let expected = service(StorageProfile::POSTGIS_LTREE)
                .query_ids(&live_target(), &query(limit, offset))
                .await
                .expect("the full profile answers");
            let deferred = StorageProfile {
                geo_predicates: GeoPredicatePushdown::BoundingBoxOnly,
                value_filter: ValueFilterPushdown::Structured,
                scope: ScopePushdown::PrefixOnly,
            };
            let actual = service(deferred)
                .query_ids(&live_target(), &query(limit, offset))
                .await
                .expect("the deferring profile answers");
            pretty_assertions::assert_eq!(actual, expected, "limit={limit} offset={offset}");
        }
    }

    #[tokio::test]
    async fn a_store_that_cannot_narrow_a_predicate_refuses_rather_than_answering_it_wrongly() {
        // Clause 5.5.2: finishing this in memory means reading every entity, so
        // the honest answer is OperationNotSupported.
        let refusing = [
            StorageProfile {
                geo_predicates: GeoPredicatePushdown::None,
                value_filter: ValueFilterPushdown::JsonPath,
                scope: ScopePushdown::LabelTree,
            },
            StorageProfile {
                geo_predicates: GeoPredicatePushdown::Full,
                value_filter: ValueFilterPushdown::None,
                scope: ScopePushdown::LabelTree,
            },
            StorageProfile {
                geo_predicates: GeoPredicatePushdown::Full,
                value_filter: ValueFilterPushdown::JsonPath,
                scope: ScopePushdown::None,
            },
        ];
        for profile in refusing {
            let error = service(profile).query_ids(&live_target(), &query(100, 0)).await.expect_err("must refuse");
            assert!(
                error.to_string().contains("cannot evaluate"),
                "the refusal explains itself rather than answering slowly: {error}"
            );
        }
    }

    #[tokio::test]
    async fn a_predicate_the_query_does_not_carry_never_refuses_the_query() {
        // An axis says what the store can do, not what the query asked for.
        let no_geo_support = StorageProfile {
            geo_predicates: GeoPredicatePushdown::None,
            value_filter: ValueFilterPushdown::JsonPath,
            scope: ScopePushdown::LabelTree,
        };
        let without_geo = CompiledEntityQuery::builder()
            .selection(
                EntitySelection::builder()
                    .q_expr(Some(parser::parse("\"https://example.com/temperature\">15").expect("valid q filter")))
                    .build(),
            )
            .pagination(Pagination {
                limit: 100,
                offset: 0,
                count: CountPolicy::Omit,
            })
            .build();

        let ids = service(no_geo_support)
            .query_ids(&live_target(), &without_geo)
            .await
            .expect("no geo predicate, no refusal");
        pretty_assertions::assert_eq!(ids.len(), 5, "every building above 15 degrees");
    }
}
