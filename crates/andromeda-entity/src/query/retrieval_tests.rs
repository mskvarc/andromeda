//! Clause 5.6.3 retrieval and clause 5.7.2 query over a store holding one
//! entity.
//!
//! The store answers the one read a retrieval or a query page makes, the
//! state together with its system times, and refuses every other, so a read
//! path that reached for a second read fails here.

use crate::{
    configured_policy::{filter_complexity_limits, geometry_census_limits},
    entity_snapshot::EntitySnapshot,
    error::EntityError,
    pagination_outcome::PaginationOutcome,
    port::{query::EntityQueryStore, reader::EntityReader, retrieved_entity::RetrievedEntity},
    query::{
        EntityQueryService,
        entity_versions::{VersionRead, held_version},
        retrieval::RetrievalNarrowing,
    },
    query_outcome::QueryOutcome,
    query_residual::ResidualFilter,
    spatial_acceleration::SpatialAcceleration,
    storage_profile::StorageProfile,
};
use andromeda_configuration::schema::request_limits::RequestLimitsConfig;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{entity::Entity, notification::SysAttrsSnapshot};
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity::compiled::CompiledEntityQuery,
    filter::language_targeting::LanguageTargeting,
    geo::{admission::GeoQueryAdmission, near_distance_band::NearDistanceBand},
    page_window::PageWindow,
    projection::{compiled::Projection, returned_attributes::ReturnedAttributes, term::ProjectionTerm},
    type_selection::grammar::TypeExprRaw,
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use andromeda_tenancy::tenant_ref::TenantRef;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::json;
use std::{collections::HashMap, slice::from_ref, sync::Arc};

const VEHICLE: &str = "urn:ngsi-ld:Vehicle:1";

/// A store holding one entity and answering only the retrieval read.
struct RetrievalStore {
    retrieved: RetrievedEntity,
}

/// The failure a read other than the retrieval read reports.
fn unexpected(method: &str) -> EntityError {
    panic!("RetrievalStore::{method}: a retrieval reads the state and its system times in one read")
}

#[async_trait]
impl EntityReader for RetrievalStore {
    async fn get(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<Arc<EntityState>>, EntityError> {
        Err(unexpected("get"))
    }

    async fn get_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, RetrievedEntity>, EntityError> {
        Err(unexpected("get_batch"))
    }

    async fn get_with_sys_attrs(
        &self,
        _target: &ContextTarget,
        id: &Identifier,
        _returned: &ReturnedAttributes<'_>,
    ) -> Result<Option<RetrievedEntity>, EntityError> {
        // clone: the store hands out its row, as a real store does
        Ok((self.retrieved.state.id() == id).then(|| self.retrieved.clone()))
    }

    async fn get_snapshot(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<EntitySnapshot>, EntityError> {
        Err(unexpected("get_snapshot"))
    }

    async fn get_snapshot_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, EntitySnapshot>, EntityError> {
        Err(unexpected("get_snapshot_batch"))
    }

    async fn get_sys_attrs(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<SysAttrsSnapshot>, EntityError> {
        Err(unexpected("get_sys_attrs"))
    }

    async fn get_sys_attrs_batch(&self, _target: &ContextTarget, _ids: &[Identifier]) -> Result<HashMap<Identifier, SysAttrsSnapshot>, EntityError> {
        Err(unexpected("get_sys_attrs_batch"))
    }
}

#[async_trait]
impl EntityQueryStore for RetrievalStore {
    fn profile(&self) -> &StorageProfile {
        &StorageProfile::POSTGIS_LTREE
    }

    async fn query_compiled(
        &self,
        _target: &ContextTarget,
        _compiled: &CompiledEntityQuery,
        _window: &PageWindow,
        _count: CountPolicy,
    ) -> Result<QueryOutcome<RetrievedEntity>, EntityError> {
        Ok(QueryOutcome {
            rows: vec![self.retrieved.clone()], // clone: the store hands out its row, as a real store does
            unapplied: ResidualFilter::default(),
            pagination: PaginationOutcome::Applied { total: None },
        })
    }

    async fn query_ids(&self, _target: &ContextTarget, _compiled: &CompiledEntityQuery, _window: &PageWindow) -> Result<QueryOutcome<Identifier>, EntityError> {
        Err(unexpected("query_ids"))
    }
}

fn instant(text: &str) -> DateTime<Utc> {
    text.parse().expect("an RFC 3339 instant")
}

fn sys_attrs() -> SysAttrsSnapshot {
    SysAttrsSnapshot {
        created_at: instant("2026-01-01T00:00:00Z"),
        modified_at: instant("2026-02-01T00:00:00Z"),
        expires_at: None,
    }
}

/// A Vehicle in `/Madrid` with a `speed` and a `name`.
fn service() -> EntityQueryService {
    let document = json!([{
        "@id": VEHICLE,
        "@type": ["https://example.com/Vehicle"],
        "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/Madrid"}],
        "https://example.com/speed": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 80}]}],
        "https://example.com/name": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "Bus"}]}]
    }]);
    let state = EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored state");
    let store = Arc::new(RetrievalStore {
        retrieved: RetrievedEntity {
            state: Arc::new(state),
            sys_attrs: sys_attrs(),
        },
    });
    EntityQueryService::builder()
        .reader(Arc::clone(&store) as Arc<dyn EntityReader>) // clone: the double supplies both focused capabilities
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

fn context() -> ContextSpec {
    ContextSpec::from_body(
        JsonLiteral::admit(
            &json!({
                "Vehicle": "https://example.com/Vehicle",
                "speed": "https://example.com/speed",
                "name": "https://example.com/name",
                "brand": "https://example.com/brand"
            })
            .to_string(),
        )
        .expect("an admissible literal"),
    )
}

fn id(value: &str) -> Identifier {
    value.parse().expect("a valid identifier")
}

fn name(value: &str) -> AttributeName {
    value.parse().expect("a valid attribute name")
}

fn narrowing<'a>(attrs: &'a [AttributeName], sys_attrs: SysAttrsInclusion, projection: Option<&'a Projection>) -> RetrievalNarrowing<'a> {
    RetrievalNarrowing {
        entity_type: None,
        attrs,
        sys_attrs,
        dataset_ids: &[],
        projection,
    }
}

/// Retrieve `id` (clause 5.6.3) from the one-entity store, narrowed to what
/// the request selected and compacted under the test context.
async fn compacted_retrieval(id: &Identifier, narrowing: &RetrievalNarrowing<'_>) -> Result<Entity, EntityError> {
    let service = service();
    let context = context();
    let held = service.read_held(&ContextTarget::Live, id, narrowing, &context).await?;
    let read = VersionRead {
        id_override: Some(id),
        sys_attrs: narrowing.sys_attrs,
        dataset_ids: narrowing.dataset_ids,
    };
    let mut entities = service.compact_versions(from_ref(&held), &read, &context).await?;
    entities.pop().ok_or_else(|| EntityError::not_found(id))
}

async fn retrieve(narrowing: &RetrievalNarrowing<'_>) -> Result<Entity, EntityError> {
    compacted_retrieval(&id(VEHICLE), narrowing).await
}

/// The entity's attribute names, sorted: JSON object member order is not
/// significant (IETF RFC 8259 section 4).
fn attribute_names(entity: &Entity) -> Vec<&str> {
    let mut names: Vec<&str> = entity.attrs.keys().map(AttributeName::as_str).collect();
    names.sort_unstable();
    names
}

/// Clause 4.8, design defect 9.7: the entity-level `createdAt` and
/// `modifiedAt` come from the read that returned the state.
#[tokio::test]
async fn a_retrieval_with_sys_attrs_carries_the_times_read_with_the_state() {
    let entity = retrieve(&narrowing(&[], SysAttrsInclusion::Include, None)).await.expect("retrieved");
    pretty_assertions::assert_eq!(entity.created_at, Some(instant("2026-01-01T00:00:00Z")));
    pretty_assertions::assert_eq!(entity.modified_at, Some(instant("2026-02-01T00:00:00Z")));
}

/// Table 5.2.14-1: without `sysAttrs` the entity carries no system times.
#[tokio::test]
async fn a_retrieval_without_sys_attrs_carries_no_entity_times() {
    let entity = retrieve(&narrowing(&[], SysAttrsInclusion::Strip, None)).await.expect("retrieved");
    assert!(entity.created_at.is_none());
    assert!(entity.modified_at.is_none());
    pretty_assertions::assert_eq!(attribute_names(&entity), vec!["name", "speed"]);
}

/// Clause 5.7.2 `attrs`: the entity keeps the named attribute, compacted
/// under the request context, and, as "a synonym for the pick parameter,
/// except that id, type, scope are not allowed" (Table 6.5.3.1-1), loses its
/// `scope`.
#[tokio::test]
async fn attrs_narrow_the_retrieved_entity_to_the_named_attributes() {
    let attrs = [name("speed")];
    let entity = retrieve(&narrowing(&attrs, SysAttrsInclusion::Strip, None)).await.expect("retrieved");
    pretty_assertions::assert_eq!(attribute_names(&entity), vec!["speed"]);
    assert!(entity.scope.is_none());
}

/// Clause 5.7.1.4: `attrs` naming none of the entity's attributes is
/// `ResourceNotFound`.
#[tokio::test]
async fn attrs_naming_no_attribute_of_the_entity_is_not_found() {
    let attrs = [name("brand")];
    let error = retrieve(&narrowing(&attrs, SysAttrsInclusion::Strip, None)).await.expect_err("not found");
    assert!(matches!(error, EntityError::NotFound { .. }), "{error}");
}

/// Clause 5.7.1.4 raises `ResourceNotFound` only for an `attrs` list: a pick
/// keeping only `id` leaves an entity with no attribute, and that entity is
/// still the answer.
#[tokio::test]
async fn a_pick_keeping_only_the_id_answers_with_an_entity_without_attributes() {
    let projection = Projection {
        pick: vec![ProjectionTerm::Id],
        omit: Vec::new(),
    };
    let entity = retrieve(&narrowing(&[], SysAttrsInclusion::Strip, Some(&projection))).await.expect("retrieved");
    assert!(!entity.has_attribute_members());
    assert!(entity.scope.is_none(), "a pick not naming scope drops it");
}

/// Clause 4.21 with clause 5.7.1.5: a pick naming nothing the entity has
/// leaves nothing to answer with.
#[tokio::test]
async fn a_pick_naming_nothing_the_entity_has_is_not_found() {
    let projection = Projection {
        pick: vec![ProjectionTerm::Attribute {
            iri: "https://example.com/brand".parse().expect("valid IRI"),
            linked: Vec::new(),
        }],
        omit: Vec::new(),
    };
    let error = retrieve(&narrowing(&[], SysAttrsInclusion::Strip, Some(&projection)))
        .await
        .expect_err("not found");
    assert!(matches!(error, EntityError::NotFound { .. }), "{error}");
}

/// Clause 5.7.1.4: an entity whose types fall outside the `type` selector is
/// not the target.
#[tokio::test]
async fn an_entity_outside_the_type_selector_is_not_found() {
    let building = TypeExprRaw::Type("Building".parse().expect("a valid type name"));
    let outside = RetrievalNarrowing {
        entity_type: Some(&building),
        ..narrowing(&[], SysAttrsInclusion::Strip, None)
    };
    let error = retrieve(&outside).await.expect_err("not found");
    assert!(matches!(error, EntityError::NotFound { .. }), "{error}");

    let vehicle = TypeExprRaw::Type("Vehicle".parse().expect("a valid type name"));
    let inside = RetrievalNarrowing {
        entity_type: Some(&vehicle),
        ..narrowing(&[], SysAttrsInclusion::Strip, None)
    };
    retrieve(&inside).await.expect("the entity carries the selected type");
}

/// Clause 5.6.3: an identifier the store does not hold is not found.
#[tokio::test]
async fn an_entity_the_store_does_not_hold_is_not_found() {
    let error = compacted_retrieval(&id("urn:ngsi-ld:Vehicle:2"), &narrowing(&[], SysAttrsInclusion::Strip, None))
        .await
        .expect_err("not found");
    assert!(matches!(error, EntityError::NotFound { .. }), "{error}");
}

/// A clause 5.7.2 query over the one-entity store, its page compacted as a
/// query response writes it.
async fn query(parameters: &str) -> Vec<Entity> {
    let parameters: EntityQueryParameters = serde_urlencoded::from_str(parameters).expect("valid query parameters");
    let service = service();
    let (rows, _) = service
        .query_states(
            &ContextTarget::Live,
            &parameters,
            &PageWindow::offset(20, 0),
            CountPolicy::Omit,
            &context(),
            LanguageTargeting::Specification,
        )
        .await
        .expect("queried");
    let versions: Vec<_> = rows.into_iter().map(held_version).collect();
    let read = VersionRead {
        id_override: None,
        sys_attrs: parameters.parse_options().expect("options").sys_attrs,
        dataset_ids: &[],
    };
    service.compact_versions(&versions, &read, &context()).await.expect("compacted")
}

/// Clause 4.8, design defect 9.7: a query page's entity-level `createdAt` and
/// `modifiedAt` come from the page statement that returned the states, not
/// from a second read.
#[tokio::test]
async fn a_query_page_with_sys_attrs_carries_the_times_read_with_each_state() {
    let entities = query("type=Vehicle&options=sysAttrs").await;
    pretty_assertions::assert_eq!(entities.len(), 1);
    pretty_assertions::assert_eq!(entities[0].created_at, Some(instant("2026-01-01T00:00:00Z")));
    pretty_assertions::assert_eq!(entities[0].modified_at, Some(instant("2026-02-01T00:00:00Z")));
}

/// Table 5.2.14-1: without `sysAttrs` a query page carries no system times.
#[tokio::test]
async fn a_query_page_without_sys_attrs_carries_no_entity_times() {
    let entities = query("type=Vehicle").await;
    pretty_assertions::assert_eq!(entities.len(), 1);
    assert!(entities[0].created_at.is_none());
    assert!(entities[0].modified_at.is_none());
    pretty_assertions::assert_eq!(attribute_names(&entities[0]), vec!["name", "speed"]);
}
