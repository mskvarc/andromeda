//! An entity holding a geometry of a million positions is written to a
//! response body without a tree of its positions (clause 4.7): the normalized
//! and the `GeoJSON` renderings peak below 64 MiB above what was allocated
//! before them, not counting the body itself, where a JSON tree of the same
//! positions would take several hundred.
//!
//! The measurement counts the allocations of the calling thread alone, so the
//! other tests of the binary do not disturb it.

use andromeda_http_headers::media_type::ResponseMediaType;
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_geometry::{
    form::{GeometryForm, PolygonRings},
    geometry::Geometry,
    linear_ring::LinearRing,
    position::{Position, PositionError},
};
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
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
use andromeda_ngsi_ld_render::{
    context::ResponseContext,
    entity_response::{EntityBody, EntityPayload},
    error::RenderError,
    response::ResponseRenderer,
};
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use std::io::{Result as IoResult, Write};

/// The positions of the test polygon's one ring, closing position included.
const POSITIONS: usize = 1_000_000;

/// The rendering may peak this far above what was allocated before it.
const PEAK_BUDGET: u64 = 64 * 1024 * 1024;

/// A body that counts the bytes written into it and keeps none, so the peak
/// measured is the rendering's own and not the body's.
#[derive(Default)]
struct CountingBody {
    /// Bytes written so far.
    written: usize,
}

impl Write for CountingBody {
    fn write(&mut self, buffer: &[u8]) -> IoResult<usize> {
        self.written += buffer.len();
        Ok(buffer.len())
    }

    fn flush(&mut self) -> IoResult<()> {
        Ok(())
    }
}

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

/// An entity whose `location` is the Polygon of the one ring `ring`.
fn polygon_entity(id: Identifier, entity_type: EntityTypeName, ring: LinearRing) -> Entity {
    let location = GeoProperty {
        value: GeoPropertyValue::Geometry(Geometry::new(GeometryForm::Polygon(PolygonRings::new(vec![ring])))),
        unit_code: None,
        value_type: None,
        previous_value: None,
        common: CommonAttributeFields::default(),
    };
    Entity::builder()
        .id(id)
        .type_(EntityTypeValue::Single(entity_type))
        .location(Some(AttributeValue::Single(Box::new(Attribute::GeoProperty(location)))))
        .build()
}

/// The peak above the allocation before it, and the bytes written, of
/// rendering `entity` as `media_type` under `context`.
fn rendering_peak(entity: &Entity, media_type: ResponseMediaType, context: JsonLiteral) -> Result<(u64, usize), RenderError> {
    let mut body = CountingBody::default();
    let mut rendered = None;
    let measured = allocation_counter::measure(|| {
        rendered = Some(
            ResponseRenderer::new(media_type, ResponseContext::Body(context))
                .with_format(ResponseFormat::Normalized)
                .with_entities(EntityPayload::Single(EntityBody::new(entity, None)), None)
                .render_into(&mut body),
        );
    });
    if let Some(Err(refusal)) = rendered {
        return Err(refusal);
    }
    Ok((measured.bytes_max, body.written))
}

#[test]
fn a_million_position_geometry_renders_without_a_tree_of_its_positions() {
    let ring = LinearRing::try_new(ring_positions().expect("positions on the CRS84 axes")).expect("a closed ring");
    let entity = polygon_entity("urn:ngsi-ld:Area:1".parse().expect("an identifier"), "Area".parse().expect("a type name"), ring);
    for media_type in [ResponseMediaType::JsonLd, ResponseMediaType::GeoJson] {
        let (peak, written) = rendering_peak(
            &entity,
            media_type,
            JsonLiteral::string("https://example.org/ctx.jsonld").expect("a JSON string"),
        )
        .expect("the entity renders");
        // Every position is written: at least two ordinates of three characters each.
        assert!(written > POSITIONS * 6, "{media_type:?}: only {written} bytes were written");
        assert!(
            peak < PEAK_BUDGET,
            "{media_type:?}: rendering {POSITIONS} positions peaked at {peak} bytes above the entity, over the {PEAK_BUDGET} byte budget"
        );
    }
}
