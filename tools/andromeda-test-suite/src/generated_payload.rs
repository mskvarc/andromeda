//! Text of bodies too large to write out, drawn for a budget of the stack: a closed polygon ring of a given number of
//! positions, a run of spaces, and a text placed as one JSON string.

use googletest::Result as TestResult;
use ngsi_ld::model::{
    Number,
    Value as ModelValue,
    geometry::{LinearRing, Polygon, Position, Shape},
};
use serde_json::Value;
use std::{f64::consts::TAU, iter, num::TryFromIntError};
use thiserror::Error;

/// The point every drawn ring is centred on, inside it.
const CENTRE: (f64, f64) = (14.55, 46.05);

/// The radius of every drawn ring, in degrees.
const RADIUS: f64 = 0.05;

/// The fraction digits the ordinates of a drawn ring are rounded to.
const ORDINATE_DIGITS: usize = 9;

/// The fewest positions a closed linear ring holds (RFC 7946 section 3.1.6).
const MINIMUM_RING_POSITIONS: u64 = 4;

/// Why a body cannot be drawn.
#[derive(Debug, Error)]
pub enum PayloadError {
    /// The ring was asked for fewer positions than a closed linear ring holds.
    #[error("a closed ring holds {MINIMUM_RING_POSITIONS} positions at least, {0} were asked for")]
    TooFewPositions(u64),
    /// The count does not fit the platform's sizes.
    #[error("the count does not fit in memory")]
    Count(#[from] TryFromIntError),
}

/// The ordinates of a closed ring of `positions` positions on a circle around [`CENTRE`], each rounded to
/// [`ORDINATE_DIGITS`] fraction digits and written in its shortest form; the last position repeats the first.
fn ring_ordinates(positions: u64) -> Result<Vec<(String, String)>, PayloadError> {
    if positions < MINIMUM_RING_POSITIONS {
        return Err(PayloadError::TooFewPositions(positions));
    }
    let vertices = usize::try_from(positions - 1)?;
    let vertex = |k: usize| {
        #[expect(clippy::cast_precision_loss, reason = "ring sizes stay far below 2^52")]
        let angle = TAU * k as f64 / vertices as f64;
        (ordinate(CENTRE.0 + RADIUS * angle.cos()), ordinate(CENTRE.1 + RADIUS * angle.sin()))
    };
    Ok((0..vertices).chain(iter::once(0)).map(vertex).collect())
}

/// `value` rounded to [`ORDINATE_DIGITS`] fraction digits and written in its shortest form, with at least one fraction
/// digit.
fn ordinate(value: f64) -> String {
    let rounded = format!("{value:.ORDINATE_DIGITS$}");
    let trimmed = rounded.trim_end_matches('0');
    match trimmed.strip_suffix('.') {
        Some(whole) => format!("{whole}.0"),
        None => trimmed.to_owned(),
    }
}

/// The `GeoJSON` coordinates of a polygon holding one ring of `positions` positions, `[[...]]` (RFC 7946 section
/// 3.1.6).
///
/// # Errors
///
/// Returns [`PayloadError`] when a closed ring cannot hold that many positions.
pub fn polygon_coordinates(positions: u64) -> Result<String, PayloadError> {
    let ring = ring_ordinates(positions)?
        .iter()
        .map(|(longitude, latitude)| format!("[{longitude},{latitude}]"))
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!("[[{ring}]]"))
}

/// A `GeoJSON` Polygon object whose one ring holds `positions` positions.
///
/// # Errors
///
/// Returns [`PayloadError`] when a closed ring cannot hold that many positions.
pub fn polygon(positions: u64) -> Result<String, PayloadError> {
    Ok(format!(r#"{{"type":"Polygon","coordinates":{}}}"#, polygon_coordinates(positions)?))
}

/// The polygon [`polygon`] writes, as the model's shape.
///
/// # Errors
///
/// Returns [`PayloadError`] when a closed ring cannot hold that many positions, or the error of the geometry when a
/// position or the ring is refused.
pub fn ring_polygon(positions: u64) -> TestResult<Shape> {
    let ring = ring_ordinates(positions)?
        .iter()
        .map(|(longitude, latitude)| Ok(Position::try_new(longitude.parse()?, latitude.parse()?)?))
        .collect::<TestResult<Vec<_>>>()?;
    Ok(Shape::Polygon(Polygon::try_new(vec![LinearRing::try_new(ring)?])?))
}

/// A list of `count` sevens, a Property value of `count` JSON values.
///
/// # Errors
///
/// Returns the error of the number when `7` is refused.
pub fn sevens(count: u64) -> TestResult<ModelValue> {
    Ok(ModelValue::Array(vec![ModelValue::Number("7".parse::<Number>()?); usize::try_from(count)?]))
}

/// `count` spaces: a body of valid JSON whitespace whose only subject is its length.
///
/// # Errors
///
/// Returns [`PayloadError`] when `count` does not fit in memory.
pub fn spaces(count: u64) -> Result<String, PayloadError> {
    Ok(" ".repeat(usize::try_from(count)?))
}

/// `text` as one JSON string, quotes and escapes included.
#[must_use]
pub fn json_string(text: &str) -> String {
    Value::String(text.to_owned()).to_string()
}

#[cfg(test)]
mod tests {
    use crate::generated_payload::{PayloadError, json_string, polygon, polygon_coordinates, ring_polygon, sevens, spaces};
    use googletest::prelude::*;
    use ngsi_ld::model::{Value as ModelValue, geometry::Shape};

    #[gtest]
    fn a_polygon_holds_one_closed_ring_of_the_asked_positions() -> Result<()> {
        let coordinates: Vec<Vec<Vec<f64>>> = serde_json::from_str(&polygon_coordinates(6)?)?;
        let ring = coordinates.first().or_fail()?;
        expect_that!(coordinates.len(), eq(1));
        expect_that!(ring.len(), eq(6));
        expect_that!(ring.first(), eq(ring.last()));
        verify_that!(polygon(6)?, starts_with(r#"{"type":"Polygon","coordinates":[[["#))
    }

    #[gtest]
    fn ordinates_are_written_shortest_with_one_fraction_digit_at_least() -> Result<()> {
        verify_that!(polygon_coordinates(5)?, starts_with("[[[14.6,46.05],"))
    }

    #[gtest]
    fn a_ring_of_fewer_than_four_positions_is_refused() {
        expect_that!(matches!(polygon_coordinates(3), Err(PayloadError::TooFewPositions(3))), eq(true));
    }

    #[gtest]
    fn spaces_are_counted() -> Result<()> {
        verify_that!(spaces(4)?, eq("    "))
    }

    #[gtest]
    fn a_drawn_polygon_holds_the_asked_positions_in_one_closed_ring() -> Result<()> {
        let Shape::Polygon(polygon) = ring_polygon(6)? else {
            return fail!("the drawn shape is not a polygon");
        };
        expect_that!(polygon.rings().len(), eq(1));
        verify_that!(polygon.rings().first().map(|ring| ring.positions().len()), some(eq(6)))
    }

    #[gtest]
    fn a_list_of_sevens_holds_the_asked_count() -> Result<()> {
        let value = sevens(3)?;
        let ModelValue::Array(items) = &value else {
            return fail!("the value is not a list");
        };
        verify_that!(items.len(), eq(3))
    }

    #[gtest]
    fn a_text_is_placed_as_one_json_string() {
        expect_that!(json_string(r#"[1,"a"]"#), eq(r#""[1,\"a\"]""#));
    }
}
