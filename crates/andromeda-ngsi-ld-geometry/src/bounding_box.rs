//! The `bbox` member of a `GeoJSON` geometry (IETF RFC 7946 section 5).
//!
//! "The value of the bbox member MUST be an array of length 2*n where n is the
//! number of dimensions represented in the contained geometries, with all axes
//! of the most southwesterly point followed by all axes of the more
//! northeasterly point." Each corner is a CRS84 position held to the rule every
//! position is held to, and the south-west latitude may not lie north of the
//! north-east one (section 5.2). The longitudes are not ordered: a box crossing
//! the antimeridian has its west longitude greater than its east one.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.7.2 admits a geometry "meeting the syntax and
//! restrictions mandated by IETF RFC 7946", and clause 4.6.4 returns the content
//! of a value as provided, so a box is checked when written and kept.

use crate::{
    coordinate_tape::{CoordinateTape, TapeItem},
    position::{Position, PositionError},
};
use strum::Display;
use thiserror::Error;

/// A `GeoJSON` bounding box: its south-west corner, then its north-east corner,
/// each a CRS84 position of the same arity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundingBox {
    /// The south-west corner, altitude included when the box has three axes.
    south_west: Position,
    /// The north-east corner, of the same arity.
    north_east: Position,
}

/// Why a `bbox` member is no RFC 7946 bounding box.
#[derive(Clone, Copy, Debug, Error, PartialEq)]
pub enum BoundingBoxError {
    /// The member is not an array of numbers.
    #[error("a GeoJSON `bbox` must be an array of numbers")]
    NotNumbers,
    /// The array holds neither two corners of two axes nor two of three.
    #[error("a GeoJSON `bbox` holds {members} numbers; RFC 7946 section 5 requires 2*n, 4 for two dimensions or 6 for three")]
    Length {
        /// How many numbers the array held.
        members: usize,
    },
    /// A corner is no CRS84 position.
    #[error("the {corner} corner of a GeoJSON `bbox` is no position: {source}")]
    Corner {
        /// Which corner.
        corner: BoxCorner,
        /// What is wrong with it.
        #[source]
        source: PositionError,
    },
    /// The south-west latitude lies north of the north-east one.
    #[error("a GeoJSON `bbox` places its south-west latitude {south} north of its north-east latitude {north}; RFC 7946 section 5.2")]
    LatitudeOrder {
        /// The south-west latitude.
        south: f64,
        /// The north-east latitude.
        north: f64,
    },
}

/// A corner of a bounding box.
#[derive(Clone, Copy, Debug, Display, PartialEq, Eq)]
#[strum(serialize_all = "kebab-case")]
pub enum BoxCorner {
    /// The most south-westerly point, written first.
    SouthWest,
    /// The more north-easterly point, written second.
    NorthEast,
}

impl BoundingBox {
    /// Read the numbers of a `bbox` member, in written order.
    ///
    /// # Errors
    ///
    /// Returns [`BoundingBoxError::Length`] for any count but 4 or 6,
    /// [`BoundingBoxError::Corner`] for a corner off the CRS84 axes or with a
    /// non-finite altitude, and [`BoundingBoxError::LatitudeOrder`] for a
    /// south-west latitude north of the north-east one.
    pub fn try_from_ordinates(ordinates: &[f64]) -> Result<BoundingBox, BoundingBoxError> {
        let (south_west, north_east) = match ordinates {
            [west, south, east, north] => ((*west, *south, None), (*east, *north, None)),
            [west, south, low, east, north, high] => ((*west, *south, Some(*low)), (*east, *north, Some(*high))),
            other => return Err(BoundingBoxError::Length { members: other.len() }),
        };
        let corner = |corner, (longitude, latitude, altitude)| {
            Position::try_new(longitude, latitude, altitude).map_err(|source| BoundingBoxError::Corner { corner, source })
        };
        let south_west = corner(BoxCorner::SouthWest, south_west)?;
        let north_east = corner(BoxCorner::NorthEast, north_east)?;
        if south_west.latitude() > north_east.latitude() {
            return Err(BoundingBoxError::LatitudeOrder {
                south: south_west.latitude(),
                north: north_east.latitude(),
            });
        }
        Ok(BoundingBox { south_west, north_east })
    }

    /// Read a `bbox` member recorded onto a tape: one array of numbers.
    ///
    /// # Errors
    ///
    /// Returns [`BoundingBoxError::NotNumbers`] for anything but an array of
    /// numbers a finite double stands for, and otherwise the refusals of
    /// [`BoundingBox::try_from_ordinates`].
    pub(crate) fn of_tape(tape: &CoordinateTape) -> Result<BoundingBox, BoundingBoxError> {
        let mut reader = tape.reader();
        if reader.next_item() != Some(TapeItem::Open) {
            return Err(BoundingBoxError::NotNumbers);
        }
        let mut ordinates = Vec::with_capacity(6);
        loop {
            match reader.next_item() {
                Some(TapeItem::Ordinate(ordinate)) => ordinates.push(ordinate),
                Some(TapeItem::Close) => break,
                Some(TapeItem::Open | TapeItem::Other) | None => return Err(BoundingBoxError::NotNumbers),
            }
        }
        BoundingBox::try_from_ordinates(&ordinates)
    }

    /// The south-west corner.
    #[must_use]
    pub const fn south_west(&self) -> &Position {
        &self.south_west
    }

    /// The north-east corner.
    #[must_use]
    pub const fn north_east(&self) -> &Position {
        &self.north_east
    }

    /// The numbers in RFC 7946 section 5 order: every axis of the south-west
    /// corner, then every axis of the north-east corner.
    pub fn ordinates(&self) -> impl Iterator<Item = f64> {
        [self.south_west, self.north_east]
            .into_iter()
            .flat_map(|corner| [Some(corner.longitude()), Some(corner.latitude()), corner.altitude()])
            .flatten()
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        bounding_box::{BoundingBox, BoundingBoxError, BoxCorner},
        coordinate_tape::CoordinateTape,
        position::{Position, PositionError},
    };
    use serde_json::json;

    fn position(longitude: f64, latitude: f64, altitude: Option<f64>) -> Position {
        Position::try_new(longitude, latitude, altitude).expect("a position")
    }

    /// RFC 7946 section 5: a two-dimensional box is its two corners.
    #[test]
    fn a_planar_box_reads_its_two_corners() {
        let bbox = BoundingBox::try_from_ordinates(&[0.0, 1.0, 4.0, 5.0]).expect("a box");
        pretty_assertions::assert_eq!(bbox.south_west(), &position(0.0, 1.0, None));
        pretty_assertions::assert_eq!(bbox.north_east(), &position(4.0, 5.0, None));
        pretty_assertions::assert_eq!(bbox.ordinates().collect::<Vec<_>>(), vec![0.0, 1.0, 4.0, 5.0]);
    }

    /// RFC 7946 section 5: a three-dimensional box carries both altitudes.
    #[test]
    fn an_elevated_box_reads_its_two_corners_with_altitude() {
        let bbox = BoundingBox::try_from_ordinates(&[0.0, 1.0, -10.0, 4.0, 5.0, 120.0]).expect("a box");
        pretty_assertions::assert_eq!(bbox.south_west(), &position(0.0, 1.0, Some(-10.0)));
        pretty_assertions::assert_eq!(bbox.north_east(), &position(4.0, 5.0, Some(120.0)));
        pretty_assertions::assert_eq!(bbox.ordinates().collect::<Vec<_>>(), vec![0.0, 1.0, -10.0, 4.0, 5.0, 120.0]);
    }

    /// RFC 7946 section 5: "an array of length 2*n".
    #[test]
    fn a_box_of_any_other_length_is_refused() {
        for ordinates in [
            vec![],
            vec![1.0],
            vec![0.0, 0.0, 1.0],
            vec![0.0, 0.0, 1.0, 1.0, 2.0],
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0],
        ] {
            pretty_assertions::assert_eq!(
                BoundingBox::try_from_ordinates(&ordinates),
                Err(BoundingBoxError::Length { members: ordinates.len() }),
                "{ordinates:?}"
            );
        }
    }

    /// RFC 7946 sections 4 and 5.3: each corner lies on the CRS84 axes.
    #[test]
    fn a_box_corner_off_the_crs84_axes_is_refused() {
        pretty_assertions::assert_eq!(
            BoundingBox::try_from_ordinates(&[0.0, -91.0, 1.0, 1.0]),
            Err(BoundingBoxError::Corner {
                corner: BoxCorner::SouthWest,
                source: PositionError::LatitudeOutOfRange { latitude: -91.0 },
            })
        );
        pretty_assertions::assert_eq!(
            BoundingBox::try_from_ordinates(&[0.0, 0.0, 181.0, 1.0]),
            Err(BoundingBoxError::Corner {
                corner: BoxCorner::NorthEast,
                source: PositionError::LongitudeOutOfRange { longitude: 181.0 },
            })
        );
    }

    /// RFC 7946 section 5.2: the north-east latitude is always the greater.
    #[test]
    fn a_box_whose_south_lies_north_of_its_north_is_refused() {
        pretty_assertions::assert_eq!(
            BoundingBox::try_from_ordinates(&[0.0, 10.0, 1.0, 5.0]),
            Err(BoundingBoxError::LatitudeOrder { south: 10.0, north: 5.0 })
        );
    }

    /// RFC 7946 section 5.2: a box crossing the antimeridian has a west
    /// longitude greater than its east one.
    #[test]
    fn a_box_across_the_antimeridian_is_accepted() {
        assert!(BoundingBox::try_from_ordinates(&[177.0, -20.0, -178.0, -16.0]).is_ok());
    }

    /// A member that is no flat array of numbers is no box.
    #[test]
    fn a_tape_holding_anything_but_numbers_is_no_box() {
        for value in [json!("box"), json!(7), json!([0, "x", 1, 1]), json!([[0, 0], [1, 1]]), json!({"a": 1})] {
            pretty_assertions::assert_eq!(
                BoundingBox::of_tape(&CoordinateTape::of_text(&value.to_string()).expect("valid JSON")),
                Err(BoundingBoxError::NotNumbers),
                "{value}"
            );
        }
        pretty_assertions::assert_eq!(
            BoundingBox::of_tape(&CoordinateTape::of_text("[0, 1, 4, 5]").expect("valid JSON")),
            BoundingBox::try_from_ordinates(&[0.0, 1.0, 4.0, 5.0])
        );
    }
}
