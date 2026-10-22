//! Reading recorded `coordinates` as the nesting a geometry type requires.
//!
//! The `type` of a geometry selects how its `coordinates` nest (IETF RFC 7946
//! sections 3.1.2 through 3.1.7), and each position, line and ring is built
//! through the same constructors every other geometry path uses, refused at the
//! same place with the same reason. A member that breaks the nesting is refused
//! with the [`GeometryError`] naming what belongs there and where, never with
//! any part of the value itself.

use crate::{
    coordinate_path::CoordinatePath,
    coordinate_tape::{CoordinateTape, TapeItem, TapeReader},
    form::{GeometryForm, PolygonRings},
    geojson::{CoordinateShape, GeometryError},
    geometry_type::GeometryType,
    line_string_coordinates::LineStringCoordinates,
    linear_ring::LinearRing,
    position::{Position, PositionError},
};

impl CoordinateTape {
    /// Read the recorded `coordinates` as `geometry_type` nests them.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::NotAnArray`] for a member that breaks the
    /// nesting, [`GeometryError::Position`] for a position RFC 7946 section
    /// 3.1.1 or section 4 gives no meaning or that holds a member that is no
    /// number, [`GeometryError::LineString`] for a
    /// `LineString` of fewer than two positions,
    /// [`GeometryError::LinearRing`] for a Polygon ring that is open or holds
    /// fewer than four, and [`GeometryError::MixedArity`] for positions that
    /// disagree on altitude.
    pub fn read_as(&self, geometry_type: GeometryType) -> Result<GeometryForm, GeometryError> {
        let reader = &mut self.reader();
        let mut enclosing = Vec::new();
        let form = match geometry_type {
            GeometryType::Point => position(reader, &enclosing).map(GeometryForm::Point),
            GeometryType::MultiPoint => positions(reader, &mut enclosing).map(GeometryForm::MultiPoint),
            GeometryType::LineString => line_string(reader, &mut enclosing).map(GeometryForm::LineString),
            GeometryType::MultiLineString => each(reader, &mut enclosing, line_string).map(GeometryForm::MultiLineString),
            GeometryType::Polygon => rings(reader, &mut enclosing).map(GeometryForm::Polygon),
            GeometryType::MultiPolygon => each(reader, &mut enclosing, rings).map(GeometryForm::MultiPolygon),
        };
        form?.with_uniform_arity()
    }
}

/// Open the array the reader stands at, which sits at `enclosing` and holds
/// `expected`.
fn open(reader: &mut TapeReader<'_>, expected: CoordinateShape, enclosing: &[usize]) -> Result<(), GeometryError> {
    match reader.next_item() {
        Some(TapeItem::Open) => Ok(()),
        Some(TapeItem::Close | TapeItem::Ordinate(_) | TapeItem::Other) | None => Err(GeometryError::NotAnArray {
            expected,
            at: CoordinatePath::new(enclosing.to_vec()),
        }),
    }
}

/// Read one position, refused at `enclosing`.
///
/// The ordinates are read into a fixed buffer: a position of more than three
/// has no meaning and is refused by its count alone, once every member has
/// been found to be a number. The first member that is none is refused by its
/// index, an array among them too.
fn position(reader: &mut TapeReader<'_>, enclosing: &[usize]) -> Result<Position, GeometryError> {
    open(reader, CoordinateShape::Position, enclosing)?;
    let mut ordinates = [0.0_f64; 3];
    let mut count = 0_usize;
    loop {
        match reader.next_item() {
            Some(TapeItem::Close) | None => break,
            Some(TapeItem::Ordinate(ordinate)) => {
                if let Some(slot) = ordinates.get_mut(count) {
                    *slot = ordinate;
                }
                count += 1;
            }
            Some(TapeItem::Open | TapeItem::Other) => {
                return Err(GeometryError::Position {
                    at: CoordinatePath::new(enclosing.to_vec()),
                    source: PositionError::NonNumericOrdinate { index: count },
                });
            }
        }
    }
    let read = match ordinates.get(..count) {
        Some(read) => Position::try_from(read),
        None => Err(PositionError::TooManyOrdinates { ordinates: count }),
    };
    read.map_err(|source| GeometryError::Position {
        at: CoordinatePath::new(enclosing.to_vec()),
        source,
    })
}

/// Read a list of positions, each refused at its own index.
fn positions(reader: &mut TapeReader<'_>, enclosing: &mut Vec<usize>) -> Result<Vec<Position>, GeometryError> {
    open(reader, CoordinateShape::Positions, enclosing)?;
    let mut read = Vec::new();
    while !reader.closes() {
        enclosing.push(read.len());
        let item = position(reader, enclosing);
        enclosing.pop();
        read.push(item?);
    }
    Ok(read)
}

/// Read the positions of one `LineString`.
fn line_string(reader: &mut TapeReader<'_>, enclosing: &mut Vec<usize>) -> Result<LineStringCoordinates, GeometryError> {
    LineStringCoordinates::try_new(positions(reader, enclosing)?).map_err(|source| GeometryError::LineString {
        at: CoordinatePath::new(enclosing.clone()), // clone: the refusal owns the path of the one line it names
        source,
    })
}

/// Read one Polygon ring.
fn ring(reader: &mut TapeReader<'_>, enclosing: &mut Vec<usize>) -> Result<LinearRing, GeometryError> {
    LinearRing::try_new(positions(reader, enclosing)?).map_err(|source| GeometryError::LinearRing {
        at: CoordinatePath::new(enclosing.clone()), // clone: the refusal owns the path of the one ring it names
        source,
    })
}

/// Read the rings of one Polygon, exterior ring first.
fn rings(reader: &mut TapeReader<'_>, enclosing: &mut Vec<usize>) -> Result<PolygonRings, GeometryError> {
    each(reader, enclosing, ring).map(PolygonRings::new)
}

/// Read every member of the list the reader stands at with `read`, each one
/// level deeper.
fn each<T>(
    reader: &mut TapeReader<'_>,
    enclosing: &mut Vec<usize>,
    read: impl Fn(&mut TapeReader<'_>, &mut Vec<usize>) -> Result<T, GeometryError>,
) -> Result<Vec<T>, GeometryError> {
    open(reader, CoordinateShape::List, enclosing)?;
    let mut members = Vec::new();
    while !reader.closes() {
        enclosing.push(members.len());
        let member = read(reader, enclosing);
        enclosing.pop();
        members.push(member?);
    }
    Ok(members)
}
