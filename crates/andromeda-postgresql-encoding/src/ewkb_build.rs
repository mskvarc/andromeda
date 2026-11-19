//! Building a domain geometry from the callbacks a geozero reader emits.
//!
//! The inverse of the EWKB walk. geozero's readers describe a geometry as
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
//! Geometry types `PostGIS` supports but `GeoJSON` has no counterpart for (the
//! curves, the surfaces, the triangulated network) are refused rather than
//! ignored. geozero's default callbacks accept them silently, which would turn
//! a `CIRCULARSTRING` into a geometry that decodes to nothing at all.

use crate::ewkb_build_error::{GeometryBuildError, PostgisOnlyGeometryType};
use andromeda_ngsi_ld_geometry::{
    form::{GeometryForm, PolygonRings},
    geometry_type::GeometryType,
    line_string_coordinates::LineStringCoordinates,
    linear_ring::LinearRing,
    position::Position,
};
use geozero::{
    CoordDimensions,
    GeomProcessor,
    error::{GeozeroError, Result as GeozeroResult},
};

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
    MultiLineString(Vec<LineStringCoordinates>),
    /// A Polygon collecting finished rings, exterior ring first.
    Polygon(Vec<LinearRing>),
    /// A `MultiPolygon` collecting finished polygons.
    MultiPolygon(Vec<PolygonRings>),
}

impl PartialGeometry {
    /// The `GeoJSON` type of the geometry once it ends.
    const fn geometry_type(&self) -> GeometryType {
        match self {
            PartialGeometry::Point(_) => GeometryType::Point,
            PartialGeometry::MultiPoint(_) => GeometryType::MultiPoint,
            PartialGeometry::LineString(_) => GeometryType::LineString,
            PartialGeometry::MultiLineString(_) => GeometryType::MultiLineString,
            PartialGeometry::Polygon(_) => GeometryType::Polygon,
            PartialGeometry::MultiPolygon(_) => GeometryType::MultiPolygon,
        }
    }
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
            Some(open) => Err(GeometryBuildError::UnbalancedGeometry {
                geometry: open.geometry_type(),
            }),
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
    /// Nothing may enclose a finished geometry because clause 4.7.1 rules out
    /// the `GeometryCollection`: a ring inside a Polygon, a line inside a
    /// `MultiLineString` and a polygon inside a `MultiPolygon` are all folded by
    /// their own end callbacks, before they ever become a [`GeometryForm`].
    fn complete(&mut self, form: GeometryForm) -> Result<(), GeometryBuildError> {
        match self.stack.last() {
            Some(_enclosing) => Err(GeometryBuildError::MisplacedGeometry {
                geometry: form.geometry_type(),
            }),
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

/// The failure for a `PostGIS` geometry type `GeoJSON` cannot express.
fn unsupported(geometry: PostgisOnlyGeometryType) -> GeozeroError {
    GeometryBuildError::UnsupportedGeometryType { geometry }.into()
}

/// The failure for an end callback whose begin is missing or of another kind.
fn unbalanced(geometry: GeometryType) -> GeozeroError {
    GeometryBuildError::UnbalancedGeometry { geometry }.into()
}

impl GeomProcessor for GeometryBuilder {
    /// Always XYZ, so a reader hands over the third ordinate whenever the buffer
    /// carries one.
    fn dimensions(&self) -> CoordDimensions {
        CoordDimensions::xyz()
    }

    fn coordinate(&mut self, x: f64, y: f64, z: Option<f64>, _m: Option<f64>, _t: Option<f64>, _tm: Option<u64>, _idx: usize) -> GeozeroResult<()> {
        let position = Position::try_new(x, y, z).map_err(GeometryBuildError::from)?;
        Ok(self.record(position)?)
    }

    fn xy(&mut self, x: f64, y: f64, _idx: usize) -> GeozeroResult<()> {
        let position = Position::try_new(x, y, None).map_err(GeometryBuildError::from)?;
        Ok(self.record(position)?)
    }

    fn point_begin(&mut self, _idx: usize) -> GeozeroResult<()> {
        self.stack.push(PartialGeometry::Point(None));
        Ok(())
    }

    fn point_end(&mut self, _idx: usize) -> GeozeroResult<()> {
        let Some(PartialGeometry::Point(slot)) = self.stack.pop() else {
            return Err(unbalanced(GeometryType::Point));
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
            return Err(unbalanced(GeometryType::MultiPoint));
        };
        Ok(self.complete(GeometryForm::MultiPoint(positions))?)
    }

    fn linestring_begin(&mut self, _tagged: bool, size: usize, _idx: usize) -> GeozeroResult<()> {
        self.stack.push(PartialGeometry::LineString(Vec::with_capacity(size)));
        Ok(())
    }

    fn linestring_end(&mut self, _tagged: bool, _idx: usize) -> GeozeroResult<()> {
        let Some(PartialGeometry::LineString(positions)) = self.stack.pop() else {
            return Err(unbalanced(GeometryType::LineString));
        };
        // A line string is a ring when a Polygon encloses it and a member when a
        // MultiLineString does; only on its own is it a geometry in its own
        // right.
        let standalone = match self.stack.last_mut() {
            Some(PartialGeometry::Polygon(rings)) => {
                rings.push(LinearRing::try_new(positions).map_err(GeometryBuildError::from)?);
                None
            }
            Some(PartialGeometry::MultiLineString(line_strings)) => {
                line_strings.push(LineStringCoordinates::try_new(positions).map_err(GeometryBuildError::from)?);
                None
            }
            None => Some(LineStringCoordinates::try_new(positions).map_err(GeometryBuildError::from)?),
            Some(PartialGeometry::Point(_) | PartialGeometry::MultiPoint(_) | PartialGeometry::LineString(_) | PartialGeometry::MultiPolygon(_)) => {
                return Err(GeometryBuildError::MisplacedGeometry {
                    geometry: GeometryType::LineString,
                }
                .into());
            }
        };
        match standalone {
            Some(line_string) => Ok(self.complete(GeometryForm::LineString(line_string))?),
            None => Ok(()),
        }
    }

    fn multilinestring_begin(&mut self, size: usize, _idx: usize) -> GeozeroResult<()> {
        self.stack.push(PartialGeometry::MultiLineString(Vec::with_capacity(size)));
        Ok(())
    }

    fn multilinestring_end(&mut self, _idx: usize) -> GeozeroResult<()> {
        let Some(PartialGeometry::MultiLineString(line_strings)) = self.stack.pop() else {
            return Err(unbalanced(GeometryType::MultiLineString));
        };
        Ok(self.complete(GeometryForm::MultiLineString(line_strings))?)
    }

    fn polygon_begin(&mut self, _tagged: bool, size: usize, _idx: usize) -> GeozeroResult<()> {
        self.stack.push(PartialGeometry::Polygon(Vec::with_capacity(size)));
        Ok(())
    }

    fn polygon_end(&mut self, _tagged: bool, _idx: usize) -> GeozeroResult<()> {
        let Some(PartialGeometry::Polygon(rings)) = self.stack.pop() else {
            return Err(unbalanced(GeometryType::Polygon));
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
            ) => {
                return Err(GeometryBuildError::MisplacedGeometry {
                    geometry: GeometryType::Polygon,
                }
                .into());
            }
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
            return Err(unbalanced(GeometryType::MultiPolygon));
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
        Err(unsupported(PostgisOnlyGeometryType::CircularString))
    }

    fn compoundcurve_begin(&mut self, _size: usize, _idx: usize) -> GeozeroResult<()> {
        Err(unsupported(PostgisOnlyGeometryType::CompoundCurve))
    }

    fn curvepolygon_begin(&mut self, _size: usize, _idx: usize) -> GeozeroResult<()> {
        Err(unsupported(PostgisOnlyGeometryType::CurvePolygon))
    }

    fn multicurve_begin(&mut self, _size: usize, _idx: usize) -> GeozeroResult<()> {
        Err(unsupported(PostgisOnlyGeometryType::MultiCurve))
    }

    fn multisurface_begin(&mut self, _size: usize, _idx: usize) -> GeozeroResult<()> {
        Err(unsupported(PostgisOnlyGeometryType::MultiSurface))
    }

    fn triangle_begin(&mut self, _tagged: bool, _size: usize, _idx: usize) -> GeozeroResult<()> {
        Err(unsupported(PostgisOnlyGeometryType::Triangle))
    }

    fn polyhedralsurface_begin(&mut self, _size: usize, _idx: usize) -> GeozeroResult<()> {
        Err(unsupported(PostgisOnlyGeometryType::PolyhedralSurface))
    }

    fn tin_begin(&mut self, _size: usize, _idx: usize) -> GeozeroResult<()> {
        Err(unsupported(PostgisOnlyGeometryType::Tin))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        ewkb_build::{GeometryBuilder, unbalanced, unsupported},
        ewkb_build_error::{GeometryBuildError, PostgisOnlyGeometryType},
    };
    use andromeda_ngsi_ld_geometry::{
        form::GeometryForm,
        geometry_type::GeometryType,
        line_string_coordinates::LineStringError,
        linear_ring::LinearRingError,
        position::{Position, PositionError},
    };
    use geozero::{
        GeomProcessor,
        error::{GeozeroError, Result as GeozeroResult},
    };

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
        pretty_assertions::assert_eq!(error, GeometryBuildError::UnbalancedGeometry { geometry: GeometryType::Point });
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
        pretty_assertions::assert_eq!(error.to_string(), unbalanced(GeometryType::Polygon).to_string());
    }

    #[test]
    fn a_postgis_curve_type_is_refused_rather_than_silently_dropped() {
        let mut builder = GeometryBuilder::new();
        let error = builder.circularstring_begin(3, 0).expect_err("GeoJSON has no CircularString");
        pretty_assertions::assert_eq!(error.to_string(), unsupported(PostgisOnlyGeometryType::CircularString).to_string());
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
        pretty_assertions::assert_eq!(
            form,
            GeometryForm::Point(Position::try_new(1.0, 2.0, Some(34.5)).expect("a test position lies on the CRS84 axes"))
        );
    }

    #[test]
    fn a_stored_coordinate_past_a_pole_is_refused_rather_than_decoded() {
        // No write builds such a position, so a row holding one is
        // reported, never turned into a geometry the broker then matches on.
        let mut builder = GeometryBuilder::new();
        builder.point_begin(0).expect("a Point may begin");
        let error = builder
            .coordinate(0.0, 91.0, None, None, None, None, 0)
            .expect_err("latitude 91 is no position");
        pretty_assertions::assert_eq!(
            error.to_string(),
            GeozeroError::from(GeometryBuildError::Position(PositionError::LatitudeOutOfRange { latitude: 91.0 })).to_string()
        );
    }

    #[test]
    fn a_stored_coordinate_on_the_ends_of_both_axes_decodes() {
        let mut builder = GeometryBuilder::new();
        builder.point_begin(0).expect("a Point may begin");
        builder.xy(180.0, -90.0, 0).expect("the axis ends are positions");
        builder.point_end(0).expect("the Point is complete");
        pretty_assertions::assert_eq!(
            builder.finish().expect("a Point was built"),
            GeometryForm::Point(Position::try_new(180.0, -90.0, None).expect("the axis ends are positions"))
        );
    }

    #[test]
    fn an_absent_third_ordinate_leaves_the_position_without_an_altitude() {
        let mut builder = GeometryBuilder::new();
        feed_point(&mut builder, None).expect("a Point may be fed");
        let form = builder.finish().expect("the Point is complete");
        pretty_assertions::assert_eq!(
            form,
            GeometryForm::Point(Position::try_new(1.0, 2.0, None).expect("a test position lies on the CRS84 axes"))
        );
    }

    /// Feed one Polygon whose single ring runs through `ring`, and end it.
    fn feed_polygon(builder: &mut GeometryBuilder, ring: &[(f64, f64)]) -> GeozeroResult<()> {
        builder.polygon_begin(true, 1, 0)?;
        builder.linestring_begin(false, ring.len(), 0)?;
        for (index, (longitude, latitude)) in ring.iter().enumerate() {
            builder.coordinate(*longitude, *latitude, None, None, None, None, index)?;
        }
        builder.linestring_end(false, 0)?;
        builder.polygon_end(true, 0)
    }

    #[test]
    fn a_polygon_ring_folds_into_the_polygon_rather_than_standing_alone() {
        let mut builder = GeometryBuilder::new();
        feed_polygon(&mut builder, &[(1.0, 2.0), (5.0, 2.0), (5.0, 8.0), (1.0, 2.0)]).expect("a closed ring of four positions");
        let GeometryForm::Polygon(rings) = builder.finish().expect("the Polygon is complete") else {
            panic!("a Polygon was built");
        };
        pretty_assertions::assert_eq!(rings.rings().len(), 1);
        pretty_assertions::assert_eq!(rings.rings()[0].positions().len(), 4);
    }

    #[test]
    fn a_stored_open_ring_is_refused_rather_than_decoded() {
        // No write builds an open ring, so a row holding one is
        // reported rather than turned into a polygon the broker matches on.
        let mut builder = GeometryBuilder::new();
        let error = feed_polygon(&mut builder, &[(1.0, 2.0), (5.0, 2.0), (5.0, 8.0), (1.0, 8.0)]).expect_err("the ring is open");
        pretty_assertions::assert_eq!(
            error.to_string(),
            GeozeroError::from(GeometryBuildError::LinearRing(LinearRingError::Open)).to_string()
        );
    }

    #[test]
    fn a_stored_line_of_one_position_is_refused_rather_than_decoded() {
        let mut builder = GeometryBuilder::new();
        builder.linestring_begin(true, 1, 0).expect("a LineString may begin");
        builder.xy(1.0, 2.0, 0).expect("a line takes positions");
        let error = builder.linestring_end(true, 0).expect_err("one position traces no line");
        pretty_assertions::assert_eq!(
            error.to_string(),
            GeozeroError::from(GeometryBuildError::LineString(LineStringError::TooFewPositions { positions: 1 })).to_string()
        );
    }

    #[test]
    fn a_second_top_level_geometry_is_rejected_as_trailing() {
        let mut builder = GeometryBuilder::new();
        feed_point(&mut builder, None).expect("the first Point is a geometry");
        let error = feed_point(&mut builder, None).expect_err("only one top-level geometry fits in a buffer");
        pretty_assertions::assert_eq!(error.to_string(), GeozeroError::from(GeometryBuildError::TrailingGeometry).to_string());
    }
}
