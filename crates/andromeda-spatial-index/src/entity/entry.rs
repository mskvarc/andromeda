//! One entity's geometry, as the tree holds it.

use crate::partitioned_rtree::Indexed;
use andromeda_ngsi_ld_geometry::{
    form::GeometryForm,
    great_circle::great_circle_bounds,
    planar::{PlanarGeometry, PlanarPoint},
};
use andromeda_ngsi_ld_lexicon::identifier_spelling::IdentifierSpelling;
use rstar::{AABB, RTreeObject};
use std::borrow::Cow;

/// A single entity's geometry, indexed under one
/// [`SpatialKey`](crate::key::SpatialKey).
///
/// The tree holds its entries inline in its leaves, and most located entities
/// are a single `Point`, so a point entry is its identifier and its two
/// coordinates, 24 bytes. Any other geometry keeps its projection and envelope
/// behind one box, so its entry is no larger.
///
/// The envelope is the geometry's great-circle bounds rather than its planar
/// bounding rectangle. `near` measures along great-circle edges, which bow past
/// the vertices of a long edge, and an entry whose box stopped at its vertices
/// would be missing from a `near` candidate set its bulge belongs in. The box
/// still contains the planar rectangle, so the planar relations lose nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct SpatialEntry(EntryForm);

/// An entry as its geometry lets it be held.
///
/// The identifier's handle is never null, which leaves room to tell the two
/// forms apart without a tag of their own.
#[derive(Debug, Clone, PartialEq)]
enum EntryForm {
    /// A `Point`, held as its projection. Its great-circle bounds are the point
    /// itself, on the antimeridian and at a pole too, so its envelope is the
    /// degenerate box at its coordinates.
    Point {
        /// The entity the geometry belongs to.
        entity_id: IdentifierSpelling,
        /// The point's horizontal projection.
        position: PlanarPoint,
    },
    /// Any other geometry.
    Shape {
        /// The entity the geometry belongs to.
        entity_id: IdentifierSpelling,
        /// The geometry's projection and envelope.
        shape: Box<Shape>,
    },
}

/// A geometry other than a `Point`, as the tree evaluates and finds it.
#[derive(Debug, Clone, PartialEq)]
struct Shape {
    /// The horizontal projection of the geometry, projected once here rather
    /// than at every query the entry is a candidate for.
    planar: PlanarGeometry,
    /// Axis-aligned box covering the geometry read with great-circle edges,
    /// computed once here: the tree asks for it on every comparison.
    envelope: AABB<[f64; 2]>,
}

impl SpatialEntry {
    /// An entry for one entity, or `None` if the geometry has no bounding
    /// rectangle.
    ///
    /// The identifier is any clause 4.5.1 entity identifier, not only a URN,
    /// as the spelling the store tells entities apart by: an entity the tree
    /// could not hold would be missing from a partition the query path reads
    /// as complete. The back-index and the write records name the entity by
    /// this one allocation.
    ///
    /// An empty collection has no rectangle and therefore no way to be
    /// prefiltered; refusing to index it is what keeps the tree's answer and
    /// the exact predicate's answer the same.
    #[must_use]
    pub fn new(entity_id: IdentifierSpelling, geometry: &GeometryForm) -> Option<SpatialEntry> {
        let form = match geometry {
            GeometryForm::Point(position) => EntryForm::Point {
                entity_id,
                position: PlanarPoint::project(position),
            },
            GeometryForm::MultiPoint(_)
            | GeometryForm::LineString(_)
            | GeometryForm::MultiLineString(_)
            | GeometryForm::Polygon(_)
            | GeometryForm::MultiPolygon(_) => {
                let planar = PlanarGeometry::project(geometry);
                let rect = great_circle_bounds(&planar)?;
                let envelope = AABB::from_corners([rect.min().x, rect.min().y], [rect.max().x, rect.max().y]);
                EntryForm::Shape {
                    entity_id,
                    shape: Box::new(Shape { planar, envelope }),
                }
            }
        };
        Some(SpatialEntry(form))
    }

    /// The horizontal projection of the entity geometry, the form every
    /// clause 4.10 relation is evaluated over.
    ///
    /// A point's is built from its coordinates, which allocates nothing; any
    /// other geometry's is the one projected when the entry was made.
    #[must_use]
    pub fn planar(&self) -> Cow<'_, PlanarGeometry> {
        match &self.0 {
            EntryForm::Point { position, .. } => Cow::Owned(PlanarGeometry::from(*position)),
            EntryForm::Shape { shape, .. } => Cow::Borrowed(&shape.planar),
        }
    }
}

impl Indexed for SpatialEntry {
    type Id = IdentifierSpelling;

    fn id(&self) -> &IdentifierSpelling {
        match &self.0 {
            EntryForm::Point { entity_id, .. } | EntryForm::Shape { entity_id, .. } => entity_id,
        }
    }
}

impl RTreeObject for SpatialEntry {
    type Envelope = AABB<[f64; 2]>;

    fn envelope(&self) -> AABB<[f64; 2]> {
        match &self.0 {
            EntryForm::Point { position, .. } => AABB::from_point([position.longitude(), position.latitude()]),
            EntryForm::Shape { shape, .. } => shape.envelope,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{entity::entry::SpatialEntry, partitioned_rtree::Indexed};
    use andromeda_ngsi_ld_geometry::{
        form::{GeometryForm, PolygonRings},
        geo_rel::GeoRel,
        great_circle::great_circle_bounds,
        line_string_coordinates::LineStringCoordinates,
        linear_ring::LinearRing,
        planar::PlanarGeometry,
        position::Position,
        positive_distance::PositiveDistance,
        relation::matches,
    };
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, identifier_spelling::IdentifierSpelling};
    use pretty_assertions::assert_eq;
    use rstar::{AABB, RTreeObject};
    use std::str::FromStr;

    fn spelled(raw: &str) -> IdentifierSpelling {
        IdentifierSpelling::from(&raw.parse::<Identifier>().expect("a well-formed URI"))
    }

    fn position(longitude: f64, latitude: f64) -> Position {
        Position::try_new(longitude, latitude, None).expect("a test position lies on the CRS84 axes")
    }

    fn point(longitude: f64, latitude: f64) -> GeometryForm {
        GeometryForm::Point(position(longitude, latitude))
    }

    /// A polygon whose one ring runs through `corners` and back to the first.
    fn polygon(corners: &[(f64, f64)]) -> GeometryForm {
        let ring = corners
            .iter()
            .chain(corners.first())
            .map(|&(longitude, latitude)| position(longitude, latitude))
            .collect();
        GeometryForm::Polygon(PolygonRings::new(vec![LinearRing::try_new(ring).expect("a test ring is closed")]))
    }

    /// The box between two corners, as a polygon.
    fn rectangle(west: f64, south: f64, east: f64, north: f64) -> GeometryForm {
        polygon(&[(west, south), (east, south), (east, north), (west, north)])
    }

    /// Points on the antimeridian seam and at both poles, and an ordinary one.
    fn points() -> [GeometryForm; 6] {
        [
            point(8.8072, 53.0758),
            point(180.0, -17.0),
            point(-180.0, -17.0),
            point(45.0, 90.0),
            point(0.0, -90.0),
            GeometryForm::Point(Position::try_new(8.8072, 53.0758, Some(34.5)).expect("a test position lies on the CRS84 axes")),
        ]
    }

    /// Polygons beside Bremen, against the seam and around the north pole, and
    /// a line across the seam.
    fn shapes() -> [GeometryForm; 4] {
        [
            rectangle(8.80, 53.07, 8.81, 53.08),
            rectangle(179.0, -18.0, 180.0, -16.0),
            polygon(&[(0.0, 89.0), (90.0, 89.0), (180.0, 89.0), (-90.0, 89.0)]),
            GeometryForm::LineString(
                LineStringCoordinates::try_new(vec![position(179.5, -17.0), position(-179.5, -17.0)]).expect("a test line has two positions"),
            ),
        ]
    }

    fn near(max_distance: Option<&str>, min_distance: Option<&str>) -> GeoRel {
        GeoRel::Near {
            max_distance: max_distance.map(|metres| PositiveDistance::from_str(metres).expect("positive distance")),
            min_distance: min_distance.map(|metres| PositiveDistance::from_str(metres).expect("positive distance")),
        }
    }

    /// The box the tree finds an entry of `geometry` under: its great-circle
    /// bounds.
    fn great_circle_envelope(geometry: &GeometryForm) -> AABB<[f64; 2]> {
        let rect = great_circle_bounds(&PlanarGeometry::project(geometry)).expect("a test geometry has coordinates");
        AABB::from_corners([rect.min().x, rect.min().y], [rect.max().x, rect.max().y])
    }

    #[test]
    fn a_point_entry_is_its_identifier_and_two_coordinates() {
        assert_eq!(size_of::<SpatialEntry>(), 24);
    }

    #[test]
    fn an_entry_holds_the_projection_and_great_circle_envelope_of_its_geometry() {
        for geometry in points().iter().chain(&shapes()) {
            let entry = SpatialEntry::new(spelled("urn:ngsi-ld:Vehicle:A"), geometry).expect("a test geometry has coordinates");
            assert_eq!(entry.id(), &spelled("urn:ngsi-ld:Vehicle:A"));
            assert_eq!(entry.planar().into_owned(), PlanarGeometry::project(geometry), "{geometry:?}");
            assert_eq!(entry.envelope(), great_circle_envelope(geometry), "{geometry:?}");
        }
    }

    /// The references meet the subjects in every way: the same point, a point
    /// across the seam from another, boxes holding, crossing and missing them,
    /// and the polar cap.
    #[test]
    fn every_relation_answers_for_a_point_entry_and_a_shape_entry_as_for_their_geometries() {
        let references = [
            point(8.8072, 53.0758),
            point(-180.0, -17.0),
            point(10.0, 90.0),
            rectangle(8.80, 53.07, 8.81, 53.08),
            rectangle(8.805, 53.075, 8.815, 53.085),
            rectangle(179.5, -17.5, 180.0, -16.5),
            rectangle(-180.0, -17.5, -179.5, -16.5),
            polygon(&[(0.0, 88.0), (90.0, 88.0), (180.0, 88.0), (-90.0, 88.0)]),
            rectangle(-10.0, -10.0, 10.0, 10.0),
        ];
        let georels = [
            GeoRel::Within,
            GeoRel::Contains,
            GeoRel::Intersects,
            GeoRel::Disjoint,
            GeoRel::Equals,
            GeoRel::Overlaps,
            near(Some("500"), None),
            near(None, Some("500")),
            near(Some("200000"), Some("500")),
        ];
        for subject in points().iter().chain(&shapes()) {
            let entry = SpatialEntry::new(spelled("urn:ngsi-ld:Vehicle:A"), subject).expect("a test geometry has coordinates");
            let projected = PlanarGeometry::project(subject);
            for reference in references.iter().map(PlanarGeometry::project) {
                for georel in &georels {
                    assert_eq!(
                        matches(georel, &entry.planar(), &reference),
                        matches(georel, &projected, &reference),
                        "{georel:?} of {subject:?} to {reference:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_geometry_with_no_coordinates_makes_no_entry() {
        assert_eq!(SpatialEntry::new(spelled("urn:ngsi-ld:Vehicle:A"), &GeometryForm::MultiPoint(Vec::new())), None);
        assert_eq!(
            SpatialEntry::new(spelled("urn:ngsi-ld:Vehicle:A"), &GeometryForm::Polygon(PolygonRings::new(Vec::new()))),
            None
        );
    }
}
