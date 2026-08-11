//! Walking a domain geometry so a geozero writer can emit it.
//!
//! [`Geometry`] carries its own model — [`GeometryForm`] over [`Position`] —
//! and no longer converts to `geo::Geometry`, which has no room for the third
//! ordinate IETF RFC 7946 section 3.1.1 allows on every position. Feeding the
//! EWKB writer therefore means driving geozero's [`GeomProcessor`] callbacks
//! directly, which is also the only way a Z ordinate can reach the wire at all.
//!
//! Dimensionality is a property of the whole geometry, not of a position: the
//! walk announces XYZ only when every position carries an altitude, because a
//! WKB buffer has one type word for the whole geometry and PostGIS would fill
//! the gaps of a partly-elevated geometry with a fabricated zero.
//!
//! ## Polygon rings are closed on the way out
//!
//! RFC 7946 section 3.1.6 requires the first and last position of a linear ring
//! to be identical, but [`GeometryForm`] keeps a ring exactly as the client sent
//! it, open ring included, so that the client reads back what it wrote. That
//! verbatim form is not what belongs in a PostGIS `geometry` column: an open
//! ring is not a valid OGC Simple Features polygon, and PostGIS's WKB parser
//! does not enforce ring closure the way its WKT parser does, so an open ring is
//! accepted and stored as an *invalid* polygon. Every spatial predicate then
//! rests on whatever `ST_MakeValid` decides that shape should have been — a
//! repair chosen by the database rather than by the broker.
//!
//! So the walk closes each ring as it emits it, by repeating the ring's own
//! first position. That is the identity operation on an already-closed ring, it
//! invents no geometry that the ring did not already describe, and it never
//! touches the stored form: the GeoJSON the client reads back still has the ring
//! it sent, open or closed.

use andromeda_ngsi_ld_geometry::{
    dimension::GeometryDimension,
    form::{GeometryForm, PolygonRings},
    geometry::Geometry,
    position::Position,
};
use geozero::{CoordDimensions, GeomProcessor, GeozeroGeometry, error::Result as GeozeroResult};

/// A [`Geometry`] presented to geozero as a geometry source.
///
/// Borrowed rather than owned: encoding is a read of the geometry a store bind
/// already holds, and copying its positions to hand them to a writer would be
/// pure waste.
pub struct GeometryWalk<'a>(
    /// The geometry whose positions are emitted to the processor.
    pub &'a Geometry,
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
        process_form(self.0.form(), 0, processor)
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
        GeometryForm::LineString(positions) => {
            processor.linestring_begin(true, positions.len(), idx)?;
            process_positions(positions, processor)?;
            processor.linestring_end(true, idx)
        }
        GeometryForm::MultiLineString(line_strings) => {
            processor.multilinestring_begin(line_strings.len(), idx)?;
            line_strings.iter().enumerate().try_for_each(|(line_idx, positions)| {
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

/// Emit every ring of a polygon as an untagged line string, closed.
fn process_rings<P: GeomProcessor>(rings: &PolygonRings, processor: &mut P) -> GeozeroResult<()> {
    rings.rings().iter().enumerate().try_for_each(|(ring_idx, ring)| {
        let closing = closing_position(ring);
        processor.linestring_begin(false, ring.len() + usize::from(closing.is_some()), ring_idx)?;
        ring.iter()
            .chain(closing.iter())
            .enumerate()
            .try_for_each(|(position_idx, position)| process_position(position, position_idx, processor))?;
        processor.linestring_end(false, ring_idx)
    })
}

/// The position that has to be appended to `ring` to close it, or `None` when
/// the ring is already closed or has no positions to close.
///
/// The appended position is the ring's own first one, so closing adds no
/// geometry that the ring did not already describe.
fn closing_position(ring: &[Position]) -> Option<Position> {
    let first = ring.first()?;
    let last = ring.last()?;
    if first == last { None } else { Some(*first) }
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
mod tests {
    use super::*;

    fn flat(longitude: f64, latitude: f64) -> Position {
        Position::new(longitude, latitude, None)
    }

    fn tall(longitude: f64, latitude: f64, altitude: f64) -> Position {
        Position::new(longitude, latitude, Some(altitude))
    }

    #[test]
    fn an_open_ring_is_closed_by_repeating_its_first_position() {
        let ring = [flat(1.0, 2.0), flat(5.0, 2.0), flat(5.0, 8.0)];
        pretty_assertions::assert_eq!(closing_position(&ring), Some(flat(1.0, 2.0)));
    }

    #[test]
    fn a_closed_ring_needs_no_closing_position() {
        let ring = [flat(1.0, 2.0), flat(5.0, 2.0), flat(1.0, 2.0)];
        pretty_assertions::assert_eq!(closing_position(&ring), None);
    }

    #[test]
    fn an_empty_ring_needs_no_closing_position() {
        pretty_assertions::assert_eq!(closing_position(&[]), None);
    }

    #[test]
    fn a_ring_whose_ends_differ_only_in_altitude_is_closed() {
        let ring = [tall(1.0, 2.0, 10.0), tall(5.0, 2.0, 10.0), tall(1.0, 2.0, 20.0)];
        pretty_assertions::assert_eq!(closing_position(&ring), Some(tall(1.0, 2.0, 10.0)));
    }

    #[test]
    fn a_geometry_whose_positions_all_carry_an_altitude_walks_as_xyz() {
        let geometry = Geometry::new(GeometryForm::LineString(vec![tall(1.0, 2.0, 3.0), tall(4.0, 5.0, 6.0)]));
        assert!(GeometryWalk(&geometry).dims().z, "every position is elevated");
    }

    #[test]
    fn a_geometry_of_mixed_arity_walks_as_xy() {
        let geometry = Geometry::new(GeometryForm::LineString(vec![tall(1.0, 2.0, 3.0), flat(4.0, 5.0)]));
        assert!(!GeometryWalk(&geometry).dims().z, "a partly elevated geometry is planar");
    }
}
