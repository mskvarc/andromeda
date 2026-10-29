//! The query the tests ask and the index it is narrowed through.

use crate::{
    EntityService,
    admission_fixture::admitted_written,
    query::service_test_double::service_narrowing_through,
    spatial_acceleration::SpatialAcceleration,
    spatial_placement::LocationPlacement,
    write_kind::CreateEntity,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_geometry::{form::GeometryForm, geo_rel::GeoRel, geometry::Geometry, position::Position, positive_distance::PositiveDistance};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{
    entity::compiled::CompiledEntityQuery,
    entity_selection::{EntitySelection, EntitySelector},
    geo::{default_location_narrowing::DefaultLocationNarrowing, query::GeoQuery},
    type_selection::grammar::TypeExpr,
};
use andromeda_spatial_index::{
    entity::{entry::SpatialEntry, index::SpatialEntityIndex},
    key::SpatialKey,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use iri_rs::IriBuf;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{collections::HashMap, str::FromStr, sync::Arc};

/// The expanded type every vehicle in these tests carries.
pub(super) const VEHICLE_TYPE: &str = "https://uri.etsi.org/ngsi-ld/default-context/Vehicle";
pub(super) fn placement() -> LocationPlacement {
    LocationPlacement::new(TenantRef::UnnamedDefault).expect("the core location term is an attribute name")
}

pub(super) fn vehicle_key() -> SpatialKey {
    SpatialKey::new(
        TenantRef::UnnamedDefault,
        IriBuf::new(VEHICLE_TYPE.to_owned()).expect("a well-formed IRI"),
        placement().geoproperty().clone(),
    )
}

pub(super) fn point(longitude: f64) -> Geometry {
    Geometry::new(GeometryForm::Point(
        Position::try_new(longitude, 0.0, None).expect("a test position lies on the CRS84 axes"),
    ))
}

pub(super) fn uri(raw: &str) -> Identifier {
    raw.parse().expect("a well-formed URI")
}

/// An index whose `location` scope a rebuild seeded with one parked vehicle
/// far from the origin, so the scope is warm before any test writes.
pub(super) fn warm_index() -> Arc<SpatialEntityIndex> {
    let index = Arc::new(SpatialEntityIndex::new());
    let parked = SpatialEntry::new(Arc::new(uri("urn:ngsi-ld:Vehicle:parked")), point(90.0).form()).expect("a point has a bounding rectangle");
    let placement = placement();
    index.rebuild(
        placement.tenant(),
        placement.geoproperty(),
        HashMap::from([(vehicle_key(), vec![parked])]),
        index.current_generation(),
    );
    index
}

/// A vehicle at `longitude` on the equator, as a client writes it.
pub(super) fn vehicle_at(id: &str, longitude: f64) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [longitude, 0.0]}}
    })
}

/// `type=Vehicle&id=<id>&georel=near;maxDistance==1000&geometry=Point&coordinates=[0,0]`.
pub(super) fn near_origin(id: &str) -> Arc<CompiledEntityQuery> {
    let selection = EntitySelection {
        selectors: vec![EntitySelector {
            ids: vec![uri(id)],
            id_pattern: None,
            type_expr: Some(TypeExpr::Type(IriBuf::new(VEHICLE_TYPE.to_owned()).expect("a well-formed IRI"))),
        }],
        geo: Some(GeoQuery {
            georel: GeoRel::Near {
                max_distance: Some(PositiveDistance::from_str("1000").expect("a positive distance")),
                min_distance: None,
            },
            geometry: point(0.0),
            geoproperty: placement().geoproperty().clone(),
        }),
        ..EntitySelection::default()
    };
    Arc::new(CompiledEntityQuery::builder().selection(selection).build())
}

/// What the index hands the store about the query's default `location`
/// instances: `None` when it narrowed nothing, or the candidates it found.
///
/// Whatever it holds, the narrowing leaves the request's own identifiers and
/// the geo-query itself untouched, so the store still evaluates every
/// `location` instance carrying a `datasetId`.
pub(super) async fn narrowed_ids(spatial: SpatialAcceleration, query: &Arc<CompiledEntityQuery>) -> Option<Vec<Identifier>> {
    let narrowed = service_narrowing_through(spatial).spatial_prefilter(&ContextTarget::Live, query).await;
    assert_eq!(
        narrowed.selection.selectors[0].ids, query.selection.selectors[0].ids,
        "the request's identifiers are never narrowed"
    );
    assert_eq!(narrowed.selection.geo, query.selection.geo, "the store still evaluates the geo-query");
    match &narrowed.selection.default_location {
        DefaultLocationNarrowing::Unnarrowed => None,
        DefaultLocationNarrowing::Candidates(candidates) => Some(candidates.clone()), // clone: the answer outlives the narrowed query
    }
}

/// Create the entity `payload` describes through the entity service.
pub(super) async fn create(service: &EntityService, payload: Value) {
    let id = uri(payload["id"].as_str().expect("the payload names its entity"));
    let admitted = admitted_written::<CreateEntity>(service, payload, &ContextSpec::core_only()).await;
    service.create(&ContextTarget::Live, &id, admitted).await.expect("the create lands");
}
