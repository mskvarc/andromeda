//! A geometry of a million positions compacts into the compact entity without
//! a JSON tree of its positions (clause 4.7): the compaction's peak heap stays
//! below 64 MiB above the state, where the compact entity's own copy of
//! 1,000,000 positions of 32 bytes is about 32 MB and a JSON tree of them would
//! be several hundred.
//!
//! The measurement counts the allocations of the calling thread alone, so the
//! other tests of the binary do not disturb it; the compaction runs on a
//! current-thread runtime for that reason.

use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_compaction::entity_compactor::{EntityCompactor, EntityRead};
use andromeda_ngsi_ld_entity::{
    attribute_instances::AttributeInstances,
    attributes::Attributes,
    entity_state::EntityState,
    entity_types::EntityTypes,
    family_value::FamilyValue,
    geo_value::GeoValue,
    geometry_value::GeometryValue,
    instance::Instance,
};
use andromeda_ngsi_ld_geometry::{
    form::{GeometryForm, PolygonRings},
    linear_ring::LinearRing,
    position::{Position, PositionError},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::attribute::instance::{Attribute, AttributeValue};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use chrono::Utc;
use tokio::runtime::Builder;

/// How many positions the test polygon's ring holds, closing position included.
const POSITIONS: usize = 1_000_000;

/// The compaction may peak this far above what was allocated before it.
const PEAK_BUDGET: u64 = 64 * 1024 * 1024;

/// The positions of the test polygon's one ring, closing position included.
fn ring_positions() -> Result<Vec<Position>, PositionError> {
    let mut positions = Vec::with_capacity(POSITIONS);
    let mut angle = 0.0_f64;
    for _ in 1..POSITIONS {
        positions.push(Position::try_new(13.0 + angle.cos(), 52.0 + angle.sin(), None)?);
        angle += 0.000_006;
    }
    // The ring closes on its first position, the one at angle zero.
    positions.push(Position::try_new(14.0, 52.0, None)?);
    Ok(positions)
}

/// An entity whose `location` is a Polygon of the one ring `ring`.
fn polygon_entity(id: Identifier, entity_type: EntityTypeName, location: AttributeName, ring: LinearRing) -> EntityState {
    let geometry = GeoValue::Geometry(GeometryValue::new(GeometryForm::Polygon(PolygonRings::new(vec![ring]))));
    let instance = Instance::builder()
        .value(FamilyValue::GeoProperty {
            geometry,
            unit_code: None,
            value_type: None,
        })
        .build();
    let mut attributes = Attributes::new();
    attributes.insert(location, AttributeInstances::one(None, instance));
    EntityState::from_parts(id, EntityTypes::one(entity_type), None, None, attributes)
}

#[test]
fn a_geometry_compacts_into_the_compact_entity_without_a_tree_of_its_positions() {
    let runtime = Builder::new_current_thread().enable_all().build().expect("the runtime builds");
    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds");
    let context = ContextSpec::core_only();
    let compactor = engine.compactor(&context);
    runtime.block_on(compactor.processed_context()).expect("the core context processes");
    let typed = EntityCompactor::new(&compactor);
    let ring = LinearRing::try_new(ring_positions().expect("positions on the CRS84 axes")).expect("a closed ring");
    let state = polygon_entity(
        "urn:ngsi-ld:Area:1".parse().expect("an identifier"),
        "https://ex.org/Area".parse().expect("a type name"),
        "https://uri.etsi.org/ngsi-ld/location".parse().expect("an attribute name"),
        ring,
    );
    let read = EntityRead {
        id_override: None,
        sys_attrs: SysAttrsInclusion::Strip,
        now: Utc::now(),
    };
    let mut compacted = None;

    let measured = allocation_counter::measure(|| {
        compacted = Some(runtime.block_on(typed.entity(&state, &read)).expect("the entity compacts"));
    });

    let peak_above_state = measured.bytes_max;
    let entity = compacted.expect("the compaction ran");
    let Some(AttributeValue::Single(attribute)) = &entity.location else {
        panic!("the location is no single instance: {:?}", entity.location);
    };
    let Attribute::GeoProperty(geo_property) = attribute.as_ref() else {
        panic!("the location is no GeoProperty: {attribute:?}");
    };
    let positions = match geo_property.value.form().expect("the location holds a geometry") {
        GeometryForm::Polygon(rings) => rings.rings().iter().map(|ring| ring.positions().len()).sum::<usize>(),
        GeometryForm::Point(_)
        | GeometryForm::MultiPoint(_)
        | GeometryForm::LineString(_)
        | GeometryForm::MultiLineString(_)
        | GeometryForm::MultiPolygon(_) => 0,
    };
    assert_eq!(positions, POSITIONS);
    assert!(
        peak_above_state < PEAK_BUDGET,
        "compacting {POSITIONS} positions peaked at {peak_above_state} bytes above the state, over the {PEAK_BUDGET} byte budget"
    );
}
