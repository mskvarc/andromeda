//! Building a domain geometry from the callbacks a geozero reader emits.
//!
//! The inverse of [`crate::ewkb_walk`]. geozero's readers describe a geometry as
//! a stream of begin/coordinate/end events rather than as a value, so decoding
//! an EWKB buffer into [`GeometryForm`] means keeping the geometries that are
//! still open on a stack and folding each finished one into its parent.
//!
//! The builder always asks for XYZ. A two-dimensional buffer then yields
//! `z: None` on every coordinate, which is exactly the absent third ordinate of
//! IETF RFC 7946 section 3.1.1: a geometry that went into the database without
//! altitudes comes back without them, rather than with a column of fabricated
//! zeroes.
//!
//! Geometry types `PostGIS` supports but `GeoJSON` has no counterpart for — the
//! curves, the surfaces, the triangulated network — are refused rather than
//! ignored. geozero's default callbacks accept them silently, which would turn
//! a `CIRCULARSTRING` into a geometry that decodes to nothing at all.

use andromeda_ngsi_ld_geometry::{
    form::{GeometryForm, PolygonRings},
    position::Position,
};
use geozero::{
    CoordDimensions,
    GeomProcessor,
    error::{GeozeroError, Result as GeozeroResult},
};
use thiserror::Error;

/// Failures assembling a geometry from a reader's callbacks.
///
/// Every variant describes a callback stream that no well-formed EWKB buffer
/// produces, so each one means the buffer was malformed or held a geometry
/// outside the `GeoJSON` model.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum GeometryBuildError {
    /// A coordinate arrived while no geometry that holds positions was open.
    #[error("a coordinate arrived outside any geometry that can hold one")]
    UnexpectedCoordinate,
    /// A Point received a second position; RFC 7946 section 3.1.2 gives it one.
    #[error("a Point carried a second position")]
    RepeatedPointCoordinate,
    /// A Point ended without the single position it must carry.
    #[error("a Point ended without a position")]
    MissingPointCoordinate,
    /// A geometry ended without the matching begin, so the callback stream is
    /// not properly nested.
    #[error("a {geometry} ended without a matching begin")]
    UnbalancedGeometry {
        /// The geometry whose end had no matching begin.
        geometry: &'static str,
    },
    /// A geometry finished inside another that cannot contain it, such as a
    /// Polygon nested directly in a `MultiPoint`.
    #[error("a {geometry} ended inside a geometry that cannot contain one")]
    MisplacedGeometry {
        /// The geometry that finished in the wrong place.
        geometry: &'static str,
    },
    /// A second top-level geometry followed the first, which no single EWKB
    /// buffer encodes.
    #[error("the buffer encoded more than one top-level geometry")]
    TrailingGeometry,
    /// The callback stream ended without producing a geometry.
    #[error("the buffer encoded no geometry")]
    MissingGeometry,
    /// The buffer holds a `PostGIS` geometry type that `GeoJSON`, and therefore
    /// ETSI GS CIM 009 v1.9.1 clause 4.7, has no representation for.
    #[error("the buffer encodes a {geometry}, which GeoJSON has no geometry type for")]
    UnsupportedGeometryType {
        /// The `PostGIS` geometry type that cannot be represented.
        geometry: &'static str,
    },
    /// The buffer holds a `GeometryCollection`. `GeoJSON` defines one, but ETSI GS
    /// CIM 009 v1.9.1 clause 4.7.1 allows a `GeoProperty` every RFC 7946 geometry
    /// except that one, so no geometry the broker stores is a collection.
    #[error("the buffer encodes a GeometryCollection, which ETSI GS CIM 009 v1.9.1 clause 4.7.1 excludes from GeoProperty geometries")]
    GeometryCollectionUnsupported,
}

// Hand-written because the trait is geozero's and the error it carries is a
// string: a `GeomProcessor` callback may only fail as `GeozeroError`, so a typed
// build failure has to be rendered into one at that boundary.
impl From<GeometryBuildError> for GeozeroError {
    fn from(error: GeometryBuildError) -> GeozeroError {
        GeozeroError::Geometry(error.to_string())
    }
}

/// A geometry that has begun but not yet ended, holding what has arrived so far.
#[derive(Debug)]
enum PartialGeometry {
    /// A Point awaiting its single position.
    Point(Option<Position>),
    /// A `MultiPoint` collecting its positions.
    MultiPoint(Vec<Position>),
    /// A `LineString` collecting its positions, whether standalone, a member of a
    /// `MultiLineString`, or a polygon ring.
    LineString(Vec<Position>),
    /// A `MultiLineString` collecting finished line strings.
    MultiLineString(Vec<Vec<Position>>),
    /// A Polygon collecting finished rings, exterior ring first.
    Polygon(Vec<Vec<Position>>),
    /// A `MultiPolygon` collecting finished polygons.
    MultiPolygon(Vec<PolygonRings>),
}

/// Assembles a [`GeometryForm`] from a geozero reader's callbacks.
///
/// One builder decodes one geometry: [`GeometryBuilder::finish`] consumes it, so
/// a second buffer needs a second builder.
#[derive(Debug, Default)]
pub struct GeometryBuilder {
    /// Geometries that have begun and not yet ended, outermost first.
    stack: Vec<PartialGeometry>,
    /// The top-level geometry, once it has ended.
    finished: Option<GeometryForm>,
}

impl GeometryBuilder {
    /// An empty builder, ready to receive a reader's callbacks.
    #[must_use]
    pub const fn new() -> GeometryBuilder {
        GeometryBuilder {
            stack: Vec::new(),
            finished: None,
        }
    }

    /// The decoded geometry.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryBuildError::UnbalancedGeometry`] if a geometry is still
    /// open, and [`GeometryBuildError::MissingGeometry`] if none was ever
    /// completed.
    pub fn finish(self) -> Result<GeometryForm, GeometryBuildError> {
        match self.stack.last() {
            Some(open) => Err(GeometryBuildError::UnbalancedGeometry { geometry: partial_name(open) }),
            None => self.finished.ok_or(GeometryBuildError::MissingGeometry),
        }
    }

    /// Record a position into the geometry currently open.
    fn record(&mut self, position: Position) -> Result<(), GeometryBuildError> {
        match self.stack.last_mut() {
            Some(PartialGeometry::Point(slot)) => {
                if slot.is_some() {
                    Err(GeometryBuildError::RepeatedPointCoordinate)
                } else {
                    *slot = Some(position);
                    Ok(())
                }
            }
            Some(PartialGeometry::MultiPoint(positions) | PartialGeometry::LineString(positions)) => {
                positions.push(position);
                Ok(())
            }
            Some(PartialGeometry::MultiLineString(_) | PartialGeometry::Polygon(_) | PartialGeometry::MultiPolygon(_)) | None => {
                Err(GeometryBuildError::UnexpectedCoordinate)
            }
        }
    }

    /// Keep a finished geometry as the top-level result, which is the only place
    /// one can stand.
    ///
    /// Nothing may enclose a finished geometry now that clause 4.7.1 rules out
    /// the `GeometryCollection`: a ring inside a Polygon, a line inside a
    /// `MultiLineString` and a polygon inside a `MultiPolygon` are all folded by
    /// their own end callbacks, before they ever become a [`GeometryForm`].
    fn complete(&mut self, form: GeometryForm) -> Result<(), GeometryBuildError> {
        match self.stack.last() {
            Some(_enclosing) => Err(GeometryBuildError::MisplacedGeometry { geometry: form_name(&form) }),
            None => {
                if self.finished.is_some() {
                    Err(GeometryBuildError::TrailingGeometry)
                } else {
                    self.finished = Some(form);
                    Ok(())
                }
            }
        }
    }
}

/// The `GeoJSON` type name of a finished geometry, for diagnostics.
const fn form_name(form: &GeometryForm) -> &'static str {
    match form {
        GeometryForm::Point(_) => "Point",
        GeometryForm::MultiPoint(_) => "MultiPoint",
        GeometryForm::LineString(_) => "LineString",
        GeometryForm::MultiLineString(_) => "MultiLineString",
        GeometryForm::Polygon(_) => "Polygon",
        GeometryForm::MultiPolygon(_) => "MultiPolygon",
    }
}

/// The `GeoJSON` type name of an unfinished geometry, for diagnostics.
const fn partial_name(partial: &PartialGeometry) -> &'static str {
    match partial {
        PartialGeometry::Point(_) => "Point",
        PartialGeometry::MultiPoint(_) => "MultiPoint",
        PartialGeometry::LineString(_) => "LineString",
        PartialGeometry::MultiLineString(_) => "MultiLineString",
        PartialGeometry::Polygon(_) => "Polygon",
        PartialGeometry::MultiPolygon(_) => "MultiPolygon",
    }
}

/// The failure for a `PostGIS` geometry type `GeoJSON` cannot express.
fn unsupported(geometry: &'static str) -> GeozeroError {
    GeometryBuildError::UnsupportedGeometryType { geometry }.into()
}

/// The failure for an end callback whose begin is missing or of another kind.
fn unbalanced(geometry: &'static str) -> GeozeroError {
    GeometryBuildError::UnbalancedGeometry { geometry }.into()
}

impl GeomProcessor for GeometryBuilder {
    /// Always XYZ, so a reader hands over the third ordinate whenever the buffer
    /// carries one.
    fn dimensions(&self) -> CoordDimensions {
        CoordDimensions::xyz()
    }

    fn coordinate(&mut self, x: f64, y: f64, z: Option<f64>, _m: Option<f64>, _t: Option<f64>, _tm: Option<u64>, _idx: usize) -> GeozeroResult<()> {
        Ok(self.record(Position::new(x, y, z))?)
    }

    fn xy(&mut self, x: f64, y: f64, _idx: usize) -> GeozeroResult<()> {
        Ok(self.record(Position::new(x, y, None))?)
    }

    fn point_begin(&mut self, _idx: usize) -> GeozeroResult<()> {
        self.stack.push(PartialGeometry::Point(None));
        Ok(())
    }

    fn point_end(&mut self, _idx: usize) -> GeozeroResult<()> {
        let Some(PartialGeometry::Point(slot)) = self.stack.pop() else {
            return Err(unbalanced("Point"));
        };
        let position = slot.ok_or(GeometryBuildError::MissingPointCoordinate)?;
        Ok(self.complete(GeometryForm::Point(position))?)
    }

    fn multipoint_begin(&mut self, size: usize, _idx: usize) -> GeozeroResult<()> {
        self.stack.push(PartialGeometry::MultiPoint(Vec::with_capacity(size)));
        Ok(())
    }

    fn multipoint_end(&mut self, _idx: usize) -> GeozeroResult<()> {
        let Some(PartialGeometry::MultiPoint(positions)) = self.stack.pop() else {
            return Err(unbalanced("MultiPoint"));
        };
        Ok(self.complete(GeometryForm::MultiPoint(positions))?)
    }

    fn linestring_begin(&mut self, _tagged: bool, size: usize, _idx: usize) -> GeozeroResult<()> {
        self.stack.push(PartialGeometry::LineString(Vec::with_capacity(size)));
        Ok(())
    }

    fn linestring_end(&mut self, _tagged: bool, _idx: usize) -> GeozeroResult<()> {
        let Some(PartialGeometry::LineString(positions)) = self.stack.pop() else {
            return Err(unbalanced("LineString"));
        };
        // A line string is a ring when a Polygon encloses it and a member when a
        // MultiLineString does; only on its own is it a geometry in its own
        // right.
        let standalone = match self.stack.last_mut() {
            Some(PartialGeometry::Polygon(rings)) => {
                rings.push(positions);
                None
            }
            Some(PartialGeometry::MultiLineString(line_strings)) => {
                line_strings.push(positions);
                None
            }
            None => Some(positions),
            Some(PartialGeometry::Point(_) | PartialGeometry::MultiPoint(_) | PartialGeometry::LineString(_) | PartialGeometry::MultiPolygon(_)) => {
                return Err(GeometryBuildError::MisplacedGeometry { geometry: "LineString" }.into());
            }
        };
        match standalone {
            Some(positions) => Ok(self.complete(GeometryForm::LineString(positions))?),
            None => Ok(()),
        }
    }

    fn multilinestring_begin(&mut self, size: usize, _idx: usize) -> GeozeroResult<()> {
        self.stack.push(PartialGeometry::MultiLineString(Vec::with_capacity(size)));
        Ok(())
    }

    fn multilinestring_end(&mut self, _idx: usize) -> GeozeroResult<()> {
        let Some(PartialGeometry::MultiLineString(line_strings)) = self.stack.pop() else {
            return Err(unbalanced("MultiLineString"));
        };
        Ok(self.complete(GeometryForm::MultiLineString(line_strings))?)
    }

    fn polygon_begin(&mut self, _tagged: bool, size: usize, _idx: usize) -> GeozeroResult<()> {
        self.stack.push(PartialGeometry::Polygon(Vec::with_capacity(size)));
        Ok(())
    }

    fn polygon_end(&mut self, _tagged: bool, _idx: usize) -> GeozeroResult<()> {
        let Some(PartialGeometry::Polygon(rings)) = self.stack.pop() else {
            return Err(unbalanced("Polygon"));
        };
        let rings = PolygonRings::new(rings);
        // A polygon is a member when a MultiPolygon encloses it, and a geometry
        // in its own right anywhere else.
        let standalone = match self.stack.last_mut() {
            Some(PartialGeometry::MultiPolygon(polygons)) => {
                polygons.push(rings);
                None
            }
            None => Some(rings),
            Some(
                PartialGeometry::Point(_)
                | PartialGeometry::MultiPoint(_)
                | PartialGeometry::LineString(_)
                | PartialGeometry::MultiLineString(_)
                | PartialGeometry::Polygon(_),
            ) => return Err(GeometryBuildError::MisplacedGeometry { geometry: "Polygon" }.into()),
        };
        match standalone {
            Some(rings) => Ok(self.complete(GeometryForm::Polygon(rings))?),
            None => Ok(()),
        }
    }

    fn multipolygon_begin(&mut self, size: usize, _idx: usize) -> GeozeroResult<()> {
        self.stack.push(PartialGeometry::MultiPolygon(Vec::with_capacity(size)));
        Ok(())
    }

    fn multipolygon_end(&mut self, _idx: usize) -> GeozeroResult<()> {
        let Some(PartialGeometry::MultiPolygon(polygons)) = self.stack.pop() else {
            return Err(unbalanced("MultiPolygon"));
        };
        Ok(self.complete(GeometryForm::MultiPolygon(polygons))?)
    }

    /// Refused rather than decoded: ETSI GS CIM 009 v1.9.1 clause 4.7.1 excludes
    /// the `GeometryCollection` from `GeoProperty` geometries, so a buffer holding
    /// one is a row no write of this broker can have produced, and decoding it
    /// into something else would hide that.
    fn geometrycollection_begin(&mut self, _size: usize, _idx: usize) -> GeozeroResult<()> {
        Err(GeometryBuildError::GeometryCollectionUnsupported.into())
    }

    fn circularstring_begin(&mut self, _size: usize, _idx: usize) -> GeozeroResult<()> {
        Err(unsupported("CircularString"))
    }

    fn compoundcurve_begin(&mut self, _size: usize, _idx: usize) -> GeozeroResult<()> {
        Err(unsupported("CompoundCurve"))
    }

    fn curvepolygon_begin(&mut self, _size: usize, _idx: usize) -> GeozeroResult<()> {
        Err(unsupported("CurvePolygon"))
    }

    fn multicurve_begin(&mut self, _size: usize, _idx: usize) -> GeozeroResult<()> {
        Err(unsupported("MultiCurve"))
    }

    fn multisurface_begin(&mut self, _size: usize, _idx: usize) -> GeozeroResult<()> {
        Err(unsupported("MultiSurface"))
    }

    fn triangle_begin(&mut self, _tagged: bool, _size: usize, _idx: usize) -> GeozeroResult<()> {
        Err(unsupported("Triangle"))
    }

    fn polyhedralsurface_begin(&mut self, _size: usize, _idx: usize) -> GeozeroResult<()> {
        Err(unsupported("PolyhedralSurface"))
    }

    fn tin_begin(&mut self, _size: usize, _idx: usize) -> GeozeroResult<()> {
        Err(unsupported("Tin"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feed a complete Point through the callbacks a reader would emit.
    fn feed_point(builder: &mut GeometryBuilder, altitude: Option<f64>) -> GeozeroResult<()> {
        builder.point_begin(0)?;
        builder.coordinate(1.0, 2.0, altitude, None, None, None, 0)?;
        builder.point_end(0)
    }

    #[test]
    fn a_builder_that_saw_nothing_reports_a_missing_geometry() {
        let error = GeometryBuilder::new().finish().expect_err("no callbacks arrived");
        pretty_assertions::assert_eq!(error, GeometryBuildError::MissingGeometry);
    }

    #[test]
    fn a_geometry_left_open_is_reported_as_unbalanced() {
        let mut builder = GeometryBuilder::new();
        builder.point_begin(0).expect("a Point may begin");
        let error = builder.finish().expect_err("the Point never ended");
        pretty_assertions::assert_eq!(error, GeometryBuildError::UnbalancedGeometry { geometry: "Point" });
    }

    #[test]
    fn a_point_that_ended_without_a_coordinate_is_rejected() {
        let mut builder = GeometryBuilder::new();
        builder.point_begin(0).expect("a Point may begin");
        let error = builder.point_end(0).expect_err("the Point carried no position");
        pretty_assertions::assert_eq!(error.to_string(), GeozeroError::from(GeometryBuildError::MissingPointCoordinate).to_string());
    }

    #[test]
    fn a_coordinate_outside_any_geometry_is_rejected() {
        let mut builder = GeometryBuilder::new();
        let error = builder.coordinate(1.0, 2.0, None, None, None, None, 0).expect_err("no geometry is open");
        pretty_assertions::assert_eq!(error.to_string(), GeozeroError::from(GeometryBuildError::UnexpectedCoordinate).to_string());
    }

    #[test]
    fn an_end_callback_without_its_begin_is_rejected() {
        let mut builder = GeometryBuilder::new();
        let error = builder.polygon_end(true, 0).expect_err("no Polygon is open");
        pretty_assertions::assert_eq!(error.to_string(), unbalanced("Polygon").to_string());
    }

    #[test]
    fn a_postgis_curve_type_is_refused_rather_than_silently_dropped() {
        let mut builder = GeometryBuilder::new();
        let error = builder.circularstring_begin(3, 0).expect_err("GeoJSON has no CircularString");
        pretty_assertions::assert_eq!(error.to_string(), unsupported("CircularString").to_string());
    }

    #[test]
    fn a_geometry_collection_is_refused_at_its_begin_callback() {
        // ETSI GS CIM 009 v1.9.1 clause 4.7.1 excludes it from GeoProperty
        // geometries, so there is nothing for the builder to assemble.
        let mut builder = GeometryBuilder::new();
        let error = builder.geometrycollection_begin(2, 0).expect_err("a collection is not a GeoProperty geometry");
        pretty_assertions::assert_eq!(
            error.to_string(),
            GeozeroError::from(GeometryBuildError::GeometryCollectionUnsupported).to_string()
        );
    }

    #[test]
    fn a_third_ordinate_becomes_the_altitude_of_the_position_it_arrived_with() {
        let mut builder = GeometryBuilder::new();
        feed_point(&mut builder, Some(34.5)).expect("a Point may be fed");
        let form = builder.finish().expect("the Point is complete");
        pretty_assertions::assert_eq!(form, GeometryForm::Point(Position::new(1.0, 2.0, Some(34.5))));
    }

    #[test]
    fn an_absent_third_ordinate_leaves_the_position_without_an_altitude() {
        let mut builder = GeometryBuilder::new();
        feed_point(&mut builder, None).expect("a Point may be fed");
        let form = builder.finish().expect("the Point is complete");
        pretty_assertions::assert_eq!(form, GeometryForm::Point(Position::new(1.0, 2.0, None)));
    }

    #[test]
    fn a_polygon_ring_folds_into_the_polygon_rather_than_standing_alone() {
        let mut builder = GeometryBuilder::new();
        builder.polygon_begin(true, 1, 0).expect("a Polygon may begin");
        builder.linestring_begin(false, 3, 0).expect("a ring may begin");
        for (longitude, latitude) in [(1.0, 2.0), (5.0, 2.0), (1.0, 2.0)] {
            builder
                .coordinate(longitude, latitude, None, None, None, None, 0)
                .expect("a ring takes positions");
        }
        builder.linestring_end(false, 0).expect("a ring may end");
        builder.polygon_end(true, 0).expect("a Polygon may end");
        let GeometryForm::Polygon(rings) = builder.finish().expect("the Polygon is complete") else {
            panic!("a Polygon was built");
        };
        pretty_assertions::assert_eq!(rings.rings().len(), 1);
        pretty_assertions::assert_eq!(rings.rings()[0].len(), 3);
    }

    #[test]
    fn a_second_top_level_geometry_is_rejected_as_trailing() {
        let mut builder = GeometryBuilder::new();
        feed_point(&mut builder, None).expect("the first Point is a geometry");
        let error = feed_point(&mut builder, None).expect_err("only one top-level geometry fits in a buffer");
        pretty_assertions::assert_eq!(error.to_string(), GeozeroError::from(GeometryBuildError::TrailingGeometry).to_string());
    }
}
