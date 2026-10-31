//! Proof that declining a predicate does not change the answer.
//!
//! The residual machinery exists so a store that cannot push a predicate down
//! returns the same entities as one that can. With a single adapter whose every
//! axis is at its full value, nothing in production ever takes the deferred
//! branch, so that equality is asserted here instead, against an in-memory
//! store whose [`StorageProfile`] is a test parameter.
//!
//! The store applies the predicates it kept using a reference reading of the
//! specification written in this module, deliberately independent of
//! [`residual_filter::matches`] wherever the specification leaves room for a
//! second reading: identifiers, types, geometries and scopes are read off the
//! stored state here, and only `q` goes through the clause 4.9 evaluator both
//! sides share. When the two disagree, the profile that declines the predicate
//! returns a different set from the profile that pushes it down, and the
//! comparison fails. That is the only construction available without a second
//! backend that makes the residual and the pushdown check each other.
//!
//! [`StorageProfile`]: crate::storage_profile::StorageProfile

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        configured_policy::{filter_complexity_limits, geometry_census_limits},
        entity_snapshot::EntitySnapshot,
        error::EntityError,
        pagination_outcome::PaginationOutcome,
        port::{query::EntityQueryStore, reader::EntityReader, retrieved_entity::RetrievedEntity, stored_entry::StoredEntry},
        query::EntityQueryService,
        query_outcome::QueryOutcome,
        spatial_acceleration::SpatialAcceleration,
        storage_profile::{GeoPredicatePushdown, ScopePushdown, StorageProfile, ValueFilterPushdown},
    };
    use andromeda_configuration::schema::request_limits::RequestLimitsConfig;
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_jsonld::engine::processor::JsonLdEngine;
    use andromeda_ngsi_ld_entity::{entity_state::EntityState, scopes::Scopes};
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, planar::PlanarGeometry, positive_distance::PositiveDistance, relation::matches};
    use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier, scope::path::ScopePath};
    use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        cursor::PageCursor,
        entity::compiled::CompiledEntityQuery,
        entity_selection::{ConsideredEntities, EntitySelection},
        filter::{eval::evaluate, language_targeting::LanguageTargeting, parser},
        geo::{admission::GeoQueryAdmission, near_distance_band::NearDistanceBand, query::GeoQuery},
        page_window::{PageStart, PageWindow},
        projection::returned_attributes::ReturnedAttributes,
        queried_entity::linked_entities::NoLinkedEntities,
        scope::query::ScopeQuery,
    };
    use andromeda_tenancy::tenant_ref::TenantRef;
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use serde_json::json;
    use std::{collections::HashMap, str::FromStr, sync::Arc};

    const TEMPERATURE: &str = "https://example.com/temperature";
    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";
    const BUILDING: &str = "https://example.com/Building";

    /// The reference reading of the predicates this harness drives.
    ///
    /// Written from ETSI GS CIM 009 v1.9.1 rather than from
    /// `ResidualFilter`, so the two are independent witnesses of the same
    /// clauses: clause 4.10 for `georel`, clause 4.18 for `scopeQ`, clause
    /// 5.2.33 for the selectors; clause 4.9 `q` is the evaluator's own.
    fn reference_matches(selection: &EntitySelection, entity: &EntityState) -> bool {
        let q_holds = selection
            .q_expr
            .as_ref()
            .is_none_or(|expr| evaluate(expr, entity, NoLinkedEntities, selection.language_matching));
        let geo_holds = selection.geo.as_ref().is_none_or(|geo| {
            // Clause 4.10: one instance of the target GeoProperty meeting the
            // restriction is enough.
            let reference = geo.geometry.planar();
            entity
                .geo_property_geometries(geo.geoproperty.as_str())
                .any(|subject| matches(&geo.georel, &PlanarGeometry::project(subject), &reference))
        });
        let scopes: &[ScopePath] = entity.scope().map_or(&[], Scopes::as_slice);
        let scope_holds = selection.scope_q.as_ref().is_none_or(|scope_q| scope_q.matches(scopes));
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
                let type_ok = selector
                    .type_expr
                    .as_ref()
                    .is_none_or(|type_expr| type_expr.matches(&entity.types().as_slice().iter().map(EntityTypeName::as_str).collect::<Vec<&str>>()));
                let id_ok = selector.ids.is_empty() || selector.ids.contains(entity.id());
                type_ok && id_ok
            });
        q_holds && geo_holds && scope_holds && selectors_hold && considered_holds
    }

    /// An entity store that holds its rows in memory and answers exactly as much
    /// of a query as its profile allows.
    ///
    /// The store asks the profile what it cannot apply, strips that from the query it runs, and hands it back. A store that leaves a residual takes no window either, because a window over rows a later filter will thin is wrong (clause 5.5.9).
    struct ProfiledEntityStore {
        entities: Vec<Arc<EntityState>>,
        profile: StorageProfile,
    }

    impl ProfiledEntityStore {
        fn new(profile: StorageProfile) -> ProfiledEntityStore {
            ProfiledEntityStore { entities: fixtures(), profile }
        }

        /// The identifiers this store's own predicates admit, and what it could
        /// not apply.
        fn run(&self, compiled: &CompiledEntityQuery, window: &PageWindow) -> QueryOutcome<Identifier> {
            let unapplied = self.profile.unapplied(&compiled.selection);
            let mut pushed = compiled.selection.clone(); // clone: the store runs a narrowed copy of the caller's query
            unapplied.strip_from(&mut pushed);

            let admitted = self
                .entities
                .iter()
                .filter(|entity| reference_matches(&pushed, entity))
                .map(|entity| entity.id().clone()); // clone: the outcome owns the identifiers it returns

            if unapplied.is_empty() {
                QueryOutcome {
                    rows: window.cut(admitted.collect(), Identifier::as_str),
                    unapplied,
                    pagination: PaginationOutcome::Applied { total: None },
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
        async fn get(&self, _target: &ContextTarget, id: &Identifier) -> Result<Option<Arc<EntityState>>, EntityError> {
            Ok(self.entities.iter().find(|entity| entity.id() == id).map(Arc::clone))
        }

        async fn get_batch(&self, _target: &ContextTarget, ids: &[Identifier]) -> Result<HashMap<Identifier, RetrievedEntity>, EntityError> {
            Ok(self
                .entities
                .iter()
                .filter(|entity| ids.contains(entity.id()))
                .map(|entity| {
                    let retrieved = RetrievedEntity {
                        state: Arc::clone(entity),
                        sys_attrs: SysAttrsSnapshot {
                            created_at: DateTime::<Utc>::UNIX_EPOCH,
                            modified_at: DateTime::<Utc>::UNIX_EPOCH,
                            expires_at: None,
                        },
                    };
                    (entity.id().clone(), retrieved) // clone: the map owns its keys, as a real store's does
                })
                .collect())
        }

        async fn get_with_sys_attrs(
            &self,
            _target: &ContextTarget,
            _id: &Identifier,
            _returned: &ReturnedAttributes<'_>,
        ) -> Result<Option<RetrievedEntity>, EntityError> {
            Err(unused("get_with_sys_attrs"))
        }

        async fn get_snapshot(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<EntitySnapshot>, EntityError> {
            Err(unused("get_snapshot"))
        }

        async fn get_snapshot_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, EntitySnapshot>, EntityError> {
            Err(unused("get_snapshot_batch"))
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

        async fn query_compiled(
            &self,
            _target: &ContextTarget,
            _compiled: &CompiledEntityQuery,
            _window: &PageWindow,
            _count: CountPolicy,
        ) -> Result<QueryOutcome<StoredEntry<RetrievedEntity>>, EntityError> {
            Err(unused("query_compiled"))
        }

        async fn query_ids(
            &self,
            _target: &ContextTarget,
            compiled: &CompiledEntityQuery,
            window: &PageWindow,
        ) -> Result<QueryOutcome<Identifier>, EntityError> {
            Ok(self.run(compiled, window))
        }
    }

    fn live_target() -> ContextTarget {
        ContextTarget::Live
    }

    /// One building, at a temperature, at a position, in a scope.
    fn building(index: u32, temperature: f64, longitude: f64, latitude: f64, scope: &str) -> Arc<EntityState> {
        let document = json!([{
            "@id": format!("urn:ngsi-ld:Building:{index:03}"),
            "@type": [BUILDING],
            "https://uri.etsi.org/ngsi-ld/scope": [{"@value": scope}],
            TEMPERATURE: [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": temperature}]}],
            LOCATION: [{
                "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "@type": ["https://purl.org/geojson/vocab#Point"],
                    "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": longitude}, {"@value": latitude}]}]
                }]
            }]
        }]);
        Arc::new(EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored state"))
    }

    /// Six buildings spanning both sides of every predicate the harness drives.
    fn fixtures() -> Vec<Arc<EntityState>> {
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
            geometry: serde_json::from_value::<Geometry>(json!({"type": "Point", "coordinates": [13.4, 52.5]})).expect("valid geometry"),
            geoproperty: LOCATION.parse().expect("valid attribute name"),
        }
    }

    /// A query carrying every predicate the three declared axes cover.
    fn query() -> CompiledEntityQuery {
        CompiledEntityQuery::builder()
            .selection(
                EntitySelection::builder()
                    .q_expr(Some(parser::parse("\"https://example.com/temperature\">15").expect("valid q filter")))
                    .geo(Some(near_berlin()))
                    .scope_q(Some(ScopeQuery::parse("/Berlin/#").expect("valid scopeQ")))
                    .build(),
            )
            .build()
    }

    fn service(profile: StorageProfile) -> EntityQueryService {
        let store = Arc::new(ProfiledEntityStore::new(profile));
        EntityQueryService::builder()
            .reader(Arc::clone(&store) as Arc<dyn EntityReader>) // clone: the test store supplies both focused capabilities.
            .query_store(store as Arc<dyn EntityQueryStore>)
            .engine(JsonLdEngine::builder().allow_remote(false).build().expect("build engine"))
            .spatial(SpatialAcceleration::StoreVerified)
            .tenant(TenantRef::UnnamedDefault)
            .remote_linked(None)
            .geo_admission(GeoQueryAdmission::new(
                geometry_census_limits(&RequestLimitsConfig::default()),
                NearDistanceBand::Refused,
            ))
            .language_targeting(LanguageTargeting::Specification)
            .filter_complexity(filter_complexity_limits(&RequestLimitsConfig::default()))
            .build()
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
            .query_ids(&live_target(), &query(), &PageWindow::offset(100, 0))
            .await
            .expect("the full profile answers");
        assert!(!expected.is_empty(), "the fixtures must exercise the predicates");

        for (name, profile) in narrowing_profiles() {
            let actual = service(profile)
                .query_ids(&live_target(), &query(), &PageWindow::offset(100, 0))
                .await
                .expect("a narrowing profile answers");
            pretty_assertions::assert_eq!(actual, expected, "profile {name} disagreed with the full pushdown");
        }
    }

    #[tokio::test]
    async fn a_deferred_window_lands_on_the_same_page_the_store_would_have_returned() {
        // Clause 5.5.9: the window is over matching entities, so it must be
        // taken after the residual rather than before it, whether it is an
        // offset or a cursor (clause 5.5.9.1).
        let boundary = |id: &str| id.parse::<Identifier>().expect("valid URI");
        let windows = [
            PageWindow::offset(1, 0),
            PageWindow::offset(1, 1),
            PageWindow::offset(2, 0),
            PageWindow::offset(2, 1),
            PageWindow::offset(10, 0),
            PageWindow::offset(10, 5),
            PageWindow {
                limit: 2,
                start: PageStart::Cursor(None),
            },
            PageWindow::after(1, boundary("urn:ngsi-ld:Building:002")),
            PageWindow {
                limit: 1,
                start: PageStart::Cursor(Some(PageCursor::Before(boundary("urn:ngsi-ld:Building:006")))),
            },
            PageWindow {
                limit: 2,
                start: PageStart::Cursor(Some(PageCursor::End)),
            },
        ];
        for window in windows {
            let expected = service(StorageProfile::POSTGIS_LTREE)
                .query_ids(&live_target(), &query(), &window)
                .await
                .expect("the full profile answers");
            let deferred = StorageProfile {
                geo_predicates: GeoPredicatePushdown::BoundingBoxOnly,
                value_filter: ValueFilterPushdown::Structured,
                scope: ScopePushdown::PrefixOnly,
            };
            let actual = service(deferred)
                .query_ids(&live_target(), &query(), &window)
                .await
                .expect("the deferring profile answers");
            pretty_assertions::assert_eq!(actual, expected, "{window:?}");
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
            let error = service(profile)
                .query_ids(&live_target(), &query(), &PageWindow::offset(100, 0))
                .await
                .expect_err("must refuse");
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
            .build();

        let ids = service(no_geo_support)
            .query_ids(&live_target(), &without_geo, &PageWindow::offset(100, 0))
            .await
            .expect("no geo predicate, no refusal");
        pretty_assertions::assert_eq!(ids.len(), 5, "every building above 15 degrees");
    }
}
