//! A `GeoProperty` of a million positions expands from the positions the
//! transport read it into, without a JSON tree of its coordinates (clause 4.7):
//! the expansion's peak heap stays below 100 MiB above
//! the input, where the one copy of 1,000,000 positions of 32 bytes the
//! written document owns is about 32 MB, and it takes well under a second.
//!
//! The counting allocator is this test binary's global allocator, so it
//! measures nothing but this test.

use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_entity::{
    document::written_value::{ValueSlot, WrittenValue},
    geo_value::GeoValue,
};
use andromeda_ngsi_ld_expansion::{normalization::Normalization, typed_expander::TypedExpander, typed_expansion::TypedExpansion};
use andromeda_ngsi_ld_geometry::{
    form::{GeometryForm, PolygonRings},
    geometry::Geometry,
    linear_ring::LinearRing,
    position::Position,
};
use andromeda_ngsi_ld_model::{
    attribute::{
        common_fields::CommonAttributeFields,
        geo_property::GeoProperty,
        geo_property_value::GeoPropertyValue,
        instance::{Attribute, AttributeValue},
    },
    entity::Entity,
    entity_type::EntityTypeValue,
};
use andromeda_test_allocator::counting_allocator::CountingAllocator;
use indexmap::IndexMap;
use std::time::{Duration, Instant};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator::new();

/// The positions of the test polygon's one ring, closing position included.
const POSITIONS: usize = 1_000_000;

/// The expansion may peak this far above what was allocated before it.
const PEAK_BUDGET: usize = 100 * 1024 * 1024;

/// How long the expansion may take in an optimised build; an unoptimised one
/// is allowed ten times that.
const TIME_BUDGET: Duration = Duration::from_secs(1);

/// An entity whose `location` is a Polygon of one ring of [`POSITIONS`]
/// positions, as the transport decodes it; `None` should a part of it not be
/// one, which the construction rules out.
fn polygon_entity() -> Option<Entity> {
    let ring = (0..POSITIONS)
        .map(|index| {
            let step = if index + 1 == POSITIONS { 0 } else { index };
            let angle = f64::from(u32::try_from(step).ok()?) * 0.000_006;
            Position::try_new(13.0 + angle.cos(), 52.0 + angle.sin(), None).ok()
        })
        .collect::<Option<Vec<Position>>>()?;
    let form = GeometryForm::Polygon(PolygonRings::new(vec![LinearRing::try_new(ring).ok()?]));
    let location = GeoProperty {
        value: GeoPropertyValue::Geometry(Geometry::new(form)),
        unit_code: None,
        value_type: None,
        previous_value: None,
        common: CommonAttributeFields::default(),
    };
    Some(Entity {
        id: "urn:ngsi-ld:Area:1".parse().ok()?,
        type_: EntityTypeValue::Single("Area".parse().ok()?),
        scope: None,
        expires_at: None,
        location: Some(AttributeValue::Single(Box::new(Attribute::GeoProperty(location)))),
        observation_space: None,
        operation_space: None,
        created_at: None,
        modified_at: None,
        deleted_at: None,
        attrs: IndexMap::new(),
    })
}

#[tokio::test]
async fn a_million_position_polygon_expands_from_its_positions_without_a_tree() {
    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds");
    let context = engine.expansion_context(&ContextSpec::none()).await.expect("the core context processes");
    let entity = polygon_entity().expect("the polygon builds");
    let expander = TypedExpander::new(&context);
    let mark = ALLOCATOR.mark();
    let started = Instant::now();

    let walked = expander.entity(&entity, Normalization::Entity).expect("the polygon is admissible");

    let elapsed = started.elapsed();
    let peak_above_input = ALLOCATOR.peak_above(mark);
    println!("expanding {POSITIONS} positions peaked at {peak_above_input} bytes above the input and took {elapsed:?}");
    let TypedExpansion::Expanded(document) = walked else {
        panic!("the walk expands the polygon itself");
    };
    let location = &document.attributes()["https://uri.etsi.org/ngsi-ld/location"].instances()[0];
    let ValueSlot::Set(WrittenValue::Geometry(GeoValue::Geometry(form))) = location.value() else {
        panic!("a geometry");
    };
    assert_eq!(form.form().position_count().get(), POSITIONS);
    assert!(
        peak_above_input < PEAK_BUDGET,
        "expanding {POSITIONS} positions peaked at {peak_above_input} bytes above the input, over the {PEAK_BUDGET} byte budget"
    );
    let budget = if cfg!(debug_assertions) { TIME_BUDGET * 10 } else { TIME_BUDGET };
    assert!(elapsed < budget, "expanding {POSITIONS} positions took {elapsed:?}, over {budget:?}");
}
