//! The geometries a drawn Entity is located at and a drawn geoquery refers to, which the generated `q` filters also
//! draw their `GeoProperty` values from.

use crate::geometry_round_trip::{line, polygon};
use googletest::prelude::*;
use ngsi_ld::model::geometry::{Position, Shape};
use strum::{EnumIter, IntoStaticStr};

/// The geometries an Entity is located at and a geoquery refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter)]
pub enum PooledGeometry {
    /// The Point (0, 0), a vertex of the square (0, 0) to (10, 10).
    Origin,
    /// The Point (5, 5), inside the squares around the origin and a vertex of the square (5, 5) to (15, 15).
    Inside,
    /// The Point (5, 0), on the south edge of the square (0, 0) to (10, 10), which follows the equator.
    OnEdge,
    /// The Point (180, 0), on the antimeridian.
    Antimeridian,
    /// The Point (175, 5), east of the antimeridian.
    FarEast,
    /// The Point (-175, -5), west of the antimeridian.
    FarWest,
    /// The Point (0, 63), north of the parallel 60 and south of the arc from (-30, 60) to (30, 60), which bows north.
    UnderTheBow,
    /// The `LineString` from (0, 0) to (10, 10).
    Diagonal,
    /// The `LineString` from (170, 0) to (-170, 0), along the equator across the antimeridian.
    AcrossTheAntimeridian,
    /// The `LineString` from (-5, 5) to (5, 5), across the west edge of the square (0, 0) to (10, 10).
    Crossing,
    /// The square (0, 0) to (10, 10).
    Square,
    /// The square (-10, -10) to (10, 10).
    WideSquare,
    /// The square (5, 5) to (15, 15), overlapping the square (0, 0) to (10, 10).
    OverlappingSquare,
    /// The polygon (170, -10), (-170, -10), (-170, 10), (170, 10), 20 degrees wide around the antimeridian.
    AroundTheAntimeridian,
    /// The rectangle (-30, 50) to (30, 60).
    Bowed,
    /// The Point (-60, -30), inside the square (-70, -40) to (-50, -20).
    SouthAmerica,
    /// The square (-70, -40) to (-50, -20).
    SouthSquare,
    /// The Point (-100, 40), on the line from (-110, 40) to (-90, 40).
    NorthAmerica,
    /// The `LineString` from (-110, 40) to (-90, 40).
    Prairie,
    /// The Point (25, -25).
    Africa,
    /// The Point (140, -30).
    Australia,
    /// The Point (100, 30), inside the square (95, 25) to (110, 40) and about 700 km from (105, 35).
    Asia,
    /// The Point (105, 35), inside the square (95, 25) to (110, 40) and a vertex of the square (105, 35) to (120, 50).
    NearAsia,
    /// The square (95, 25) to (110, 40).
    AsiaSquare,
    /// The square (105, 35) to (120, 50), overlapping the square (95, 25) to (110, 40).
    AsiaOverlap,
    /// The Point (-140, 30).
    Pacific,
    /// The Point (-35, 15).
    Atlantic,
    /// The Point (55, 20).
    Arabia,
    /// The Point (75, -35).
    IndianOcean,
    /// The Point (-80, 65).
    Canada,
    /// The Point (115, -5).
    Indonesia,
    /// The Point (-120, -45).
    SouthPacific,
    /// The Point (0, -70).
    Antarctica,
}

/// The part of the world a pooled geometry lies in, more than 2000 km from every other part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Region {
    /// Around the origin, within 15 degrees of it.
    Origin,
    /// Around the antimeridian on the equator.
    Antimeridian,
    /// North of the parallel 50, around the meridian 0.
    North,
    /// Around (-60, -30).
    SouthAmerica,
    /// Around (-100, 40).
    NorthAmerica,
    /// Around (25, -25).
    Africa,
    /// Around (140, -30).
    Australia,
    /// Around (105, 35).
    Asia,
    /// At (-140, 30).
    Pacific,
    /// At (-35, 15).
    Atlantic,
    /// At (55, 20).
    Arabia,
    /// At (75, -35).
    IndianOcean,
    /// At (-80, 65).
    Canada,
    /// At (115, -5).
    Indonesia,
    /// At (-120, -45).
    SouthPacific,
    /// At (0, -70).
    Antarctica,
}

/// The type of a pooled geometry and the positions it runs through, each a longitude and a latitude.
///
/// The variant names are the `GeoJSON` type names, which a `[type]` member path reads.
#[derive(Debug, Clone, Copy, IntoStaticStr)]
pub enum Outline {
    /// A Point at this longitude and latitude, which a `[coordinates]` member path reads.
    Point(f64, f64),
    /// A `LineString` through these positions.
    LineString(&'static [(f64, f64)]),
    /// A Polygon whose exterior ring runs through these positions, the first repeated last.
    Polygon(&'static [(f64, f64)]),
}

impl Outline {
    /// The `GeoJSON` type name of the geometry.
    #[must_use]
    pub fn type_name(self) -> &'static str {
        self.into()
    }
}

impl PooledGeometry {
    /// The type of the geometry and the positions it runs through.
    #[must_use]
    pub const fn outline(self) -> Outline {
        match self {
            PooledGeometry::Origin => Outline::Point(0.0, 0.0),
            PooledGeometry::Inside => Outline::Point(5.0, 5.0),
            PooledGeometry::OnEdge => Outline::Point(5.0, 0.0),
            PooledGeometry::Antimeridian => Outline::Point(180.0, 0.0),
            PooledGeometry::FarEast => Outline::Point(175.0, 5.0),
            PooledGeometry::FarWest => Outline::Point(-175.0, -5.0),
            PooledGeometry::UnderTheBow => Outline::Point(0.0, 63.0),
            PooledGeometry::Diagonal => Outline::LineString(&[(0.0, 0.0), (10.0, 10.0)]),
            PooledGeometry::AcrossTheAntimeridian => Outline::LineString(&[(170.0, 0.0), (-170.0, 0.0)]),
            PooledGeometry::Crossing => Outline::LineString(&[(-5.0, 5.0), (5.0, 5.0)]),
            PooledGeometry::Square => Outline::Polygon(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)]),
            PooledGeometry::WideSquare => Outline::Polygon(&[(-10.0, -10.0), (10.0, -10.0), (10.0, 10.0), (-10.0, 10.0), (-10.0, -10.0)]),
            PooledGeometry::OverlappingSquare => Outline::Polygon(&[(5.0, 5.0), (15.0, 5.0), (15.0, 15.0), (5.0, 15.0), (5.0, 5.0)]),
            PooledGeometry::AroundTheAntimeridian => Outline::Polygon(&[(170.0, -10.0), (-170.0, -10.0), (-170.0, 10.0), (170.0, 10.0), (170.0, -10.0)]),
            PooledGeometry::Bowed => Outline::Polygon(&[(-30.0, 50.0), (30.0, 50.0), (30.0, 60.0), (-30.0, 60.0), (-30.0, 50.0)]),
            PooledGeometry::SouthAmerica => Outline::Point(-60.0, -30.0),
            PooledGeometry::SouthSquare => Outline::Polygon(&[(-70.0, -40.0), (-50.0, -40.0), (-50.0, -20.0), (-70.0, -20.0), (-70.0, -40.0)]),
            PooledGeometry::NorthAmerica => Outline::Point(-100.0, 40.0),
            PooledGeometry::Prairie => Outline::LineString(&[(-110.0, 40.0), (-90.0, 40.0)]),
            PooledGeometry::Africa => Outline::Point(25.0, -25.0),
            PooledGeometry::Australia => Outline::Point(140.0, -30.0),
            PooledGeometry::Asia => Outline::Point(100.0, 30.0),
            PooledGeometry::NearAsia => Outline::Point(105.0, 35.0),
            PooledGeometry::AsiaSquare => Outline::Polygon(&[(95.0, 25.0), (110.0, 25.0), (110.0, 40.0), (95.0, 40.0), (95.0, 25.0)]),
            PooledGeometry::AsiaOverlap => Outline::Polygon(&[(105.0, 35.0), (120.0, 35.0), (120.0, 50.0), (105.0, 50.0), (105.0, 35.0)]),
            PooledGeometry::Pacific => Outline::Point(-140.0, 30.0),
            PooledGeometry::Atlantic => Outline::Point(-35.0, 15.0),
            PooledGeometry::Arabia => Outline::Point(55.0, 20.0),
            PooledGeometry::IndianOcean => Outline::Point(75.0, -35.0),
            PooledGeometry::Canada => Outline::Point(-80.0, 65.0),
            PooledGeometry::Indonesia => Outline::Point(115.0, -5.0),
            PooledGeometry::SouthPacific => Outline::Point(-120.0, -45.0),
            PooledGeometry::Antarctica => Outline::Point(0.0, -70.0),
        }
    }

    /// The part of the world the geometry lies in.
    #[must_use]
    pub const fn region(self) -> Region {
        match self {
            PooledGeometry::Origin
            | PooledGeometry::Inside
            | PooledGeometry::OnEdge
            | PooledGeometry::Diagonal
            | PooledGeometry::Crossing
            | PooledGeometry::Square
            | PooledGeometry::WideSquare
            | PooledGeometry::OverlappingSquare => Region::Origin,
            PooledGeometry::Antimeridian
            | PooledGeometry::FarEast
            | PooledGeometry::FarWest
            | PooledGeometry::AcrossTheAntimeridian
            | PooledGeometry::AroundTheAntimeridian => Region::Antimeridian,
            PooledGeometry::UnderTheBow | PooledGeometry::Bowed => Region::North,
            PooledGeometry::SouthAmerica | PooledGeometry::SouthSquare => Region::SouthAmerica,
            PooledGeometry::NorthAmerica | PooledGeometry::Prairie => Region::NorthAmerica,
            PooledGeometry::Africa => Region::Africa,
            PooledGeometry::Australia => Region::Australia,
            PooledGeometry::Asia | PooledGeometry::NearAsia | PooledGeometry::AsiaSquare | PooledGeometry::AsiaOverlap => Region::Asia,
            PooledGeometry::Pacific => Region::Pacific,
            PooledGeometry::Atlantic => Region::Atlantic,
            PooledGeometry::Arabia => Region::Arabia,
            PooledGeometry::IndianOcean => Region::IndianOcean,
            PooledGeometry::Canada => Region::Canada,
            PooledGeometry::Indonesia => Region::Indonesia,
            PooledGeometry::SouthPacific => Region::SouthPacific,
            PooledGeometry::Antarctica => Region::Antarctica,
        }
    }

    /// A geometry of the pool whose interior shares part of this one's and leaves part of it out, so that `overlaps`
    /// holds between the two, when the pool holds one.
    #[must_use]
    pub const fn overlapped(self) -> Option<PooledGeometry> {
        match self {
            PooledGeometry::Square | PooledGeometry::WideSquare => Some(PooledGeometry::OverlappingSquare),
            PooledGeometry::OverlappingSquare => Some(PooledGeometry::Square),
            PooledGeometry::AsiaSquare => Some(PooledGeometry::AsiaOverlap),
            PooledGeometry::AsiaOverlap => Some(PooledGeometry::AsiaSquare),
            PooledGeometry::Origin
            | PooledGeometry::Inside
            | PooledGeometry::OnEdge
            | PooledGeometry::Antimeridian
            | PooledGeometry::FarEast
            | PooledGeometry::FarWest
            | PooledGeometry::UnderTheBow
            | PooledGeometry::Diagonal
            | PooledGeometry::AcrossTheAntimeridian
            | PooledGeometry::Crossing
            | PooledGeometry::AroundTheAntimeridian
            | PooledGeometry::Bowed
            | PooledGeometry::SouthAmerica
            | PooledGeometry::SouthSquare
            | PooledGeometry::NorthAmerica
            | PooledGeometry::Prairie
            | PooledGeometry::Africa
            | PooledGeometry::Australia
            | PooledGeometry::Asia
            | PooledGeometry::NearAsia
            | PooledGeometry::Pacific
            | PooledGeometry::Atlantic
            | PooledGeometry::Arabia
            | PooledGeometry::IndianOcean
            | PooledGeometry::Canada
            | PooledGeometry::Indonesia
            | PooledGeometry::SouthPacific
            | PooledGeometry::Antarctica => None,
        }
    }

    /// The geometry as a shape.
    ///
    /// # Errors
    ///
    /// Fails when the shape does not build.
    pub fn shape(self) -> Result<Shape> {
        Ok(match self.outline() {
            Outline::Point(longitude, latitude) => Shape::Point(Position::try_new(longitude, latitude)?),
            Outline::LineString(positions) => Shape::LineString(line(positions)?),
            Outline::Polygon(ring) => Shape::Polygon(polygon(&[ring])?),
        })
    }
}
