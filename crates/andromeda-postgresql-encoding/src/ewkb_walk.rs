//! Walking a domain geometry so a geozero writer can emit it.
//!
//! A [`GeometryForm`] is its own model, over [`Position`],
//! and does not convert to `geo::Geometry`, which has no room for the third
//! ordinate IETF RFC 7946 section 3.1.1 allows on every position. Feeding the
//! EWKB writer therefore means driving geozero's [`GeomProcessor`] callbacks
//! directly, which is also the only way a Z ordinate can reach the wire at all.
//!
//! Dimensionality is a property of the whole geometry, not of a position: the
//! walk announces XYZ only when every position carries an altitude, because a
//! WKB buffer has one type word for the whole geometry and `PostGIS` would fill
//! the gaps of a partly-elevated geometry with a fabricated zero.
//!
//! Every ring is emitted exactly as held. `PostGIS`'s WKB parser does not
//! enforce ring closure the way its WKT parser does, so an open ring would be
//! stored as an *invalid* polygon, but a [`LinearRing`] is closed by
//! construction (RFC 7946 section 3.1.6): no geometry that reaches this walk
//! has one to emit.

use andromeda_ngsi_ld_geometry::{
    dimension::GeometryDimension,
    form::{GeometryForm, PolygonRings},
    line_string_coordinates::LineStringCoordinates,
    linear_ring::LinearRing,
    position::Position,
};
use geozero::{CoordDimensions, GeomProcessor, GeozeroGeometry, error::Result as GeozeroResult};

/// A [`GeometryForm`] presented to geozero as a geometry source.
///
/// Borrowed rather than owned: encoding is a read of the geometry a store bind
/// already holds, and copying its positions to hand them to a writer would be
/// pure waste.
pub struct GeometryWalk<'a>(
    /// The geometry whose positions are emitted to the processor.
    pub &'a GeometryForm,
);

impl GeozeroGeometry for GeometryWalk<'_> {
    /// XYZ when every position of the geometry carries an altitude, XY
    /// otherwise.
    ///
    /// This is what the caller passes to `to_ewkb`, so that the writer's type
    /// word and the coordinates it receives agree.
    fn dims(&self) -> CoordDimensions {
        match self.0.dimension() {
            GeometryDimension::ThreeD => CoordDimensions::xyz(),
            GeometryDimension::TwoD => CoordDimensions::xy(),
        }
    }

    fn process_geom<P: GeomProcessor>(&self, processor: &mut P) -> GeozeroResult<()> {
        process_form(self.0, 0, processor)
    }
}

/// Emit one geometry form.
///
/// `idx` is the position of this geometry inside its parent, which geozero uses
/// to order the members of a multi-geometry.
fn process_form<P: GeomProcessor>(form: &GeometryForm, idx: usize, processor: &mut P) -> GeozeroResult<()> {
    match form {
        GeometryForm::Point(position) => {
            processor.point_begin(idx)?;
            process_position(position, 0, processor)?;
            processor.point_end(idx)
        }
        GeometryForm::MultiPoint(positions) => {
            processor.multipoint_begin(positions.len(), idx)?;
            process_positions(positions, processor)?;
            processor.multipoint_end(idx)
        }
        GeometryForm::LineString(line_string) => {
            processor.linestring_begin(true, line_string.positions().len(), idx)?;
            process_positions(line_string.positions(), processor)?;
            processor.linestring_end(true, idx)
        }
        GeometryForm::MultiLineString(line_strings) => {
            processor.multilinestring_begin(line_strings.len(), idx)?;
            line_strings
                .iter()
                .map(LineStringCoordinates::positions)
                .enumerate()
                .try_for_each(|(line_idx, positions)| {
                    processor.linestring_begin(false, positions.len(), line_idx)?;
                    process_positions(positions, processor)?;
                    processor.linestring_end(false, line_idx)
                })?;
            processor.multilinestring_end(idx)
        }
        GeometryForm::Polygon(rings) => {
            processor.polygon_begin(true, rings.rings().len(), idx)?;
            process_rings(rings, processor)?;
            processor.polygon_end(true, idx)
        }
        GeometryForm::MultiPolygon(polygons) => {
            processor.multipolygon_begin(polygons.len(), idx)?;
            polygons.iter().enumerate().try_for_each(|(polygon_idx, rings)| {
                processor.polygon_begin(false, rings.rings().len(), polygon_idx)?;
                process_rings(rings, processor)?;
                processor.polygon_end(false, polygon_idx)
            })?;
            processor.multipolygon_end(idx)
        }
    }
}

/// Emit every ring of a polygon as an untagged line string.
fn process_rings<P: GeomProcessor>(rings: &PolygonRings, processor: &mut P) -> GeozeroResult<()> {
    rings.rings().iter().map(LinearRing::positions).enumerate().try_for_each(|(ring_idx, ring)| {
        processor.linestring_begin(false, ring.len(), ring_idx)?;
        process_positions(ring, processor)?;
        processor.linestring_end(false, ring_idx)
    })
}

/// Emit a run of positions, indexed from zero.
fn process_positions<P: GeomProcessor>(positions: &[Position], processor: &mut P) -> GeozeroResult<()> {
    positions
        .iter()
        .enumerate()
        .try_for_each(|(idx, position)| process_position(position, idx, processor))
}

/// Emit one position, with its altitude when the processor asked for more than
/// two dimensions.
///
/// A processor that only wants XY gets [`GeomProcessor::xy`], which is what
/// geozero's own readers do: the two-dimensional path stays free of the option
/// juggling that the multi-dimensional one needs.
fn process_position<P: GeomProcessor>(position: &Position, idx: usize, processor: &mut P) -> GeozeroResult<()> {
    if processor.multi_dim() {
        processor.coordinate(position.longitude(), position.latitude(), position.altitude(), None, None, None, idx)
    } else {
        processor.xy(position.longitude(), position.latitude(), idx)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::ewkb_walk::GeometryWalk;
    use andromeda_ngsi_ld_geometry::{form::GeometryForm, line_string_coordinates::LineStringCoordinates, position::Position};
    use geozero::GeozeroGeometry;

    fn flat(longitude: f64, latitude: f64) -> Position {
        Position::try_new(longitude, latitude, None).expect("a test position lies on the CRS84 axes")
    }

    fn tall(longitude: f64, latitude: f64, altitude: f64) -> Position {
        Position::try_new(longitude, latitude, Some(altitude)).expect("a test position lies on the CRS84 axes")
    }

    fn line(positions: Vec<Position>) -> GeometryForm {
        GeometryForm::LineString(LineStringCoordinates::try_new(positions).expect("two positions trace a line"))
    }

    #[test]
    fn a_geometry_whose_positions_all_carry_an_altitude_walks_as_xyz() {
        let geometry = line(vec![tall(1.0, 2.0, 3.0), tall(4.0, 5.0, 6.0)]);
        assert!(GeometryWalk(&geometry).dims().z, "every position is elevated");
    }

    #[test]
    fn a_geometry_of_mixed_arity_walks_as_xy() {
        let geometry = line(vec![tall(1.0, 2.0, 3.0), flat(4.0, 5.0)]);
        assert!(!GeometryWalk(&geometry).dims().z, "a partly elevated geometry is planar");
    }
}
