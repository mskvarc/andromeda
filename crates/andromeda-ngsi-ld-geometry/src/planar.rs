//! Projection of a geometry onto the horizontal plane for evaluation.
//!
//! Altitude is dropped here, deliberately and by specification. ETSI GS CIM 009
//! v1.9.1 clause 4.10 defines `georel` by reference to OGC 06-103r4, whose
//! clause 6.1.2.5 excludes z from every spatial operation and whose clause
//! 6.1.15.1 defines the relational operators over geometries projected onto the
//! two-dimensional horizontal spatial reference plane. A
//! third ordinate therefore never enters a `georel` verdict, and the geometry
//! the evaluator sees is this projection rather than the wire form.
//!
//! `geo` is the computation engine because there is no maintained Rust crate
//! offering geographic predicates over three-dimensional coordinates: `geo` and
//! its `geo_types` foundation are two-dimensional by construction, and the
//! alternatives either wrap GEOS (which is likewise planar for these
//! predicates) or address rendering rather than geography. Rather than
//! reimplement a geometry kernel, altitude is modelled in this crate's own
//! types and evaluation stays on the projection — which is what the
//! specification asks for anyway.

use crate::{
    form::{GeometryForm, PolygonRings},
    line_string_coordinates::LineStringCoordinates,
    position::Position,
};
use geo::{Coord, Geometry as GeoGeometry, LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};

/// Project a geometry onto the horizontal plane.
///
/// The projection is a distinct value, not a view: the form it was projected
/// from keeps every altitude it arrived with.
pub fn project(form: &GeometryForm) -> GeoGeometry<f64> {
    match form {
        GeometryForm::Point(position) => GeoGeometry::Point(Point::from(coord(position))),
        GeometryForm::MultiPoint(positions) => {
            GeoGeometry::MultiPoint(MultiPoint::new(positions.iter().map(|position| Point::from(coord(position))).collect()))
        }
        GeometryForm::LineString(line) => GeoGeometry::LineString(line_string(line.positions())),
        GeometryForm::MultiLineString(lines) => GeoGeometry::MultiLineString(MultiLineString::new(
            lines.iter().map(LineStringCoordinates::positions).map(line_string).collect(),
        )),
        GeometryForm::Polygon(rings) => GeoGeometry::Polygon(polygon(rings)),
        GeometryForm::MultiPolygon(polygons) => GeoGeometry::MultiPolygon(MultiPolygon::new(polygons.iter().map(polygon).collect())),
    }
}

/// Drop the altitude of a position, keeping its horizontal ordinates.
const fn coord(position: &Position) -> Coord<f64> {
    Coord {
        x: position.longitude(),
        y: position.latitude(),
    }
}

/// Project a run of positions onto a planar line string.
fn line_string(positions: &[Position]) -> LineString<f64> {
    LineString(positions.iter().map(coord).collect())
}

/// Project the rings of a polygon, the first being the exterior ring.
///
/// A polygon with no rings projects to one with an empty exterior ring, which
/// is what `"coordinates": []` means: an area enclosing nothing.
fn polygon(rings: &PolygonRings) -> Polygon<f64> {
    let rings = rings.rings();
    let exterior = rings.first().map_or_else(|| LineString(Vec::new()), |ring| line_string(ring.positions()));
    let interiors = rings.iter().skip(1).map(|ring| line_string(ring.positions())).collect();
    Polygon::new(exterior, interiors)
}

#[cfg(test)]
mod tests {
    use crate::{
        form::{GeometryForm, PolygonRings},
        linear_ring::LinearRing,
        planar::project,
        position::Position,
    };
    use geo::{Geometry as GeoGeometry, Point};

    fn tall(longitude: f64, latitude: f64, altitude: f64) -> Position {
        Position::try_new(longitude, latitude, Some(altitude)).expect("a test position lies on the CRS84 axes")
    }

    fn ring(positions: Vec<Position>) -> LinearRing {
        LinearRing::try_new(positions).expect("a test ring is closed and long enough")
    }

    #[test]
    fn projecting_a_point_drops_its_altitude() {
        let projected = project(&GeometryForm::Point(tall(13.38, 52.52, 34.5)));
        pretty_assertions::assert_eq!(projected, GeoGeometry::Point(Point::new(13.38, 52.52)));
    }

    #[test]
    fn projecting_a_polygon_keeps_its_closed_exterior_ring() {
        let form = GeometryForm::Polygon(PolygonRings::new(vec![ring(vec![
            tall(0.0, 0.0, 1.0),
            tall(1.0, 0.0, 1.0),
            tall(1.0, 1.0, 1.0),
            tall(0.0, 0.0, 1.0),
        ])]));
        let GeoGeometry::Polygon(projected) = project(&form) else {
            panic!("expected a Polygon");
        };
        let exterior = projected.exterior();
        pretty_assertions::assert_eq!(exterior.0.len(), 4);
        pretty_assertions::assert_eq!(exterior.0.first(), exterior.0.last());
    }

    #[test]
    fn projecting_a_polygon_with_no_rings_yields_an_empty_exterior() {
        let GeoGeometry::Polygon(projected) = project(&GeometryForm::Polygon(PolygonRings::new(Vec::new()))) else {
            panic!("expected a Polygon");
        };
        assert!(projected.exterior().0.is_empty(), "an empty ring list projects to an empty exterior");
    }

    #[test]
    fn projecting_a_polygon_keeps_the_rings_after_the_first_as_interiors() {
        let exterior_ring = ring(vec![tall(0.0, 0.0, 1.0), tall(4.0, 0.0, 1.0), tall(4.0, 4.0, 1.0), tall(0.0, 0.0, 1.0)]);
        let interior_ring = ring(vec![tall(1.0, 1.0, 1.0), tall(2.0, 1.0, 1.0), tall(2.0, 2.0, 1.0), tall(1.0, 1.0, 1.0)]);
        let form = GeometryForm::Polygon(PolygonRings::new(vec![exterior_ring, interior_ring]));
        let GeoGeometry::Polygon(projected) = project(&form) else {
            panic!("expected a Polygon");
        };
        pretty_assertions::assert_eq!(projected.interiors().len(), 1);
    }
}
