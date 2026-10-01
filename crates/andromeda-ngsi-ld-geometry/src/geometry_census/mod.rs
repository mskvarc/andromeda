//! Counting what a `GeoJSON` geometry will cost before anything is built from
//! it.
//!
//! Every later form of a geometry -- its parsed tree, its typed decode, its
//! spatial projection, its `PostGIS` encoding, its index entry -- costs memory
//! per position, so the number of positions is what a budget on geometries is
//! stated over. The rest of a geometry costs memory per JSON value like any
//! other payload: its `type`, a `bbox`, the members IETF RFC 7946 section 6.1
//! calls foreign, and the lists its positions are nested in. The census counts
//! both, each against its own limit, and stops as soon as either passes.
//!
//! It reads a compact `GeoJSON` object in place, or streams a geometry a
//! client encoded as a JSON string (ETSI GS CIM 009 v1.9.1 clause 4.7.2), or
//! the bare `coordinates` a geo-query encodes as one (Table 5.2.13-1), building
//! nothing from the text. A position is an array none of whose leading members
//! is itself an array; its ordinates are part of it and are not counted as
//! values. Everything else is counted as the JSON values it holds, the way
//! [`count_values_within`](andromeda_ngsi_ld_number::json_value::value_census::count_values_within)
//! counts them.
//!
//! Whether the geometry is well formed is not the census's question: it counts
//! what it reads, and leaves every refusal of a malformed geometry to the
//! IETF RFC 7946 admission that runs afterwards.

mod census;
mod text_seeds;

use crate::geometry_census::{
    census::Census,
    text_seeds::{GeometryTextSeed, NodeSeed},
};
use andromeda_ngsi_ld_number::json_value::value_census::{JsonValueLimit, ValueTally};
use derive_more::Display;
use serde_json::Value;
use std::num::NonZeroUsize;
use thiserror::Error;

/// The member of a `GeoJSON` geometry object holding its positions (IETF RFC
/// 7946 section 3.1).
const COORDINATES_MEMBER: &str = "coordinates";

/// The most positions one geometry may hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Display)]
#[display("{_0}")]
pub struct GeometryPositionLimit(NonZeroUsize);

impl GeometryPositionLimit {
    /// A limit of `positions` positions.
    #[must_use]
    pub const fn new(positions: NonZeroUsize) -> GeometryPositionLimit {
        GeometryPositionLimit(positions)
    }

    /// The number of positions the limit admits.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0.get()
    }

    /// Whether `count` positions stay within the limit.
    #[must_use]
    pub const fn admits(self, count: PositionCount) -> bool {
        count.get() <= self.get()
    }
}

/// How many positions a geometry holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Display)]
#[display("{_0}")]
pub struct PositionCount(usize);

impl PositionCount {
    /// A count of `positions` positions.
    #[must_use]
    pub const fn new(positions: usize) -> PositionCount {
        PositionCount(positions)
    }

    /// The number of positions counted.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// The limits a geometry standing on its own is counted against: a
/// registration's geometry, or a geo-query's reference geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeometryCensusLimits {
    /// The most positions the geometry may hold.
    pub positions: GeometryPositionLimit,
    /// The most JSON values the geometry may hold besides its positions.
    pub values: JsonValueLimit,
}

impl GeometryCensusLimits {
    /// A fresh tally of the values a geometry counted against these limits
    /// holds.
    #[must_use]
    pub const fn value_tally(self) -> ValueTally {
        ValueTally::new(self.values)
    }

    /// Admit a geometry already built, holding `positions` positions.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryOverLimit::Positions`] when `positions` passes the
    /// position limit.
    pub const fn admit_positions(self, positions: PositionCount) -> Result<(), GeometryOverLimit> {
        if self.positions.admits(positions) {
            Ok(())
        } else {
            Err(GeometryOverLimit::Positions { limit: self.positions })
        }
    }
}

/// Which limit a geometry holds more than.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum GeometryOverLimit {
    /// More positions than the position limit admits.
    #[error("more than the {limit} positions this broker accepts in one geometry")]
    Positions {
        /// The limit the geometry exceeds.
        limit: GeometryPositionLimit,
    },
    /// More JSON values besides its positions than the value limit admits.
    #[error("more than the {limit} JSON values this broker accepts besides a geometry's positions")]
    Values {
        /// The limit the geometry exceeds.
        limit: JsonValueLimit,
    },
}

/// Why a geometry could not be counted within its limits.
#[derive(Debug, Error)]
pub enum GeometryCensusError {
    /// The geometry holds more than a limit admits; counting stopped at the
    /// first position or value past it.
    #[error("the geometry holds {0}")]
    OverLimit(#[from] GeometryOverLimit),
    /// A geometry or coordinates encoded as a JSON string is not a JSON text.
    #[error("the string-encoded geometry is not JSON: {0}")]
    Syntax(#[from] serde_json::Error),
}

/// Count a compact `GeoJSON` geometry: its positions against `positions`, and
/// every other value it holds into `values`.
///
/// # Errors
///
/// Returns [`GeometryCensusError::OverLimit`] once the geometry is found to
/// hold more than `positions` positions, or once `values` passes its limit.
pub fn compact_geometry_census(geometry: &Value, positions: GeometryPositionLimit, values: &ValueTally) -> Result<PositionCount, GeometryCensusError> {
    let census = Census::new(positions, values);
    census.geometry_tree(geometry).map_err(|_exceeded| census.refusal_past_a_limit())?;
    Ok(census.positions())
}

/// Count a geometry a client encoded as a JSON string (clause 4.7.2),
/// streaming the string's text and building nothing from it.
///
/// # Errors
///
/// Returns [`GeometryCensusError::OverLimit`] as [`compact_geometry_census`]
/// does, and [`GeometryCensusError::Syntax`] for a string that is not a JSON
/// text.
pub fn text_geometry_census(text: &str, positions: GeometryPositionLimit, values: &ValueTally) -> Result<PositionCount, GeometryCensusError> {
    let census = Census::new(positions, values);
    census.stream(text, GeometryTextSeed::new(&census))
}

/// Count the bare `coordinates` a geo-query encoded as a JSON string (Table
/// 5.2.13-1: "It can be encoded as a string"), streaming its text.
///
/// # Errors
///
/// Returns [`GeometryCensusError::OverLimit`] once either limit is passed, and
/// [`GeometryCensusError::Syntax`] for a string that is not a JSON text.
pub fn text_coordinates_census(text: &str, limits: GeometryCensusLimits) -> Result<PositionCount, GeometryCensusError> {
    let values = limits.value_tally();
    let census = Census::new(limits.positions, &values);
    census.stream(text, NodeSeed::new(&census))
}

#[cfg(test)]
mod tests {
    use crate::{
        geometry::Geometry,
        geometry_census::{
            GeometryCensusError,
            GeometryCensusLimits,
            GeometryOverLimit,
            GeometryPositionLimit,
            PositionCount,
            compact_geometry_census,
            text_coordinates_census,
            text_geometry_census,
        },
    };
    use andromeda_ngsi_ld_number::json_value::value_census::{JsonValueLimit, ValueTally};
    use serde_json::{Value, json};
    use std::num::NonZeroUsize;

    /// A position limit of `positions`.
    fn limit(positions: usize) -> GeometryPositionLimit {
        GeometryPositionLimit::new(NonZeroUsize::new(positions).expect("a positive test limit"))
    }

    /// A value tally of `values`.
    fn tally(values: usize) -> ValueTally {
        ValueTally::new(JsonValueLimit::new(NonZeroUsize::new(values).expect("a positive test limit")))
    }

    /// The positions and other values of a compact geometry under limits it
    /// cannot reach.
    fn compact(geometry: &Value) -> (usize, usize) {
        let values = tally(usize::MAX);
        let positions = compact_geometry_census(geometry, limit(usize::MAX), &values).expect("countable").get();
        (positions, values.counted().get())
    }

    /// The positions and other values of a string-encoded geometry under
    /// limits it cannot reach.
    fn text(geometry: &Value) -> (usize, usize) {
        let values = tally(usize::MAX);
        let positions = text_geometry_census(&geometry.to_string(), limit(usize::MAX), &values)
            .expect("countable")
            .get();
        (positions, values.counted().get())
    }

    /// A closed ring of `positions` positions.
    fn ring(positions: usize) -> Value {
        let mut points: Vec<Value> = (0..positions - 1)
            .map(|index| json!([f64::from(u32::try_from(index).expect("a small fixture index")) * 0.001, 0.5]))
            .collect();
        points.push(json!([0.0, 0.5]));
        Value::Array(points)
    }

    /// A geometry counts the same compact and string-encoded.
    fn both(geometry: &Value) -> (usize, usize) {
        let counted = compact(geometry);
        pretty_assertions::assert_eq!(text(geometry), counted, "{geometry}");
        counted
    }

    #[test]
    fn a_point_has_one_position_and_its_object_and_type_as_values() {
        pretty_assertions::assert_eq!(both(&json!({"type": "Point", "coordinates": [13.4, 52.5]})), (1, 2));
    }

    #[test]
    fn a_line_string_counts_its_positions_and_its_one_list() {
        pretty_assertions::assert_eq!(both(&json!({"type": "LineString", "coordinates": [[0, 0], [1, 1], [2, 2]]})), (3, 3));
    }

    #[test]
    fn a_polygon_counts_every_ring() {
        // object, type, the ring list, two rings
        pretty_assertions::assert_eq!(both(&json!({"type": "Polygon", "coordinates": [ring(5), ring(5)]})), (10, 5));
    }

    #[test]
    fn a_multi_polygon_sums_its_polygons() {
        pretty_assertions::assert_eq!(both(&json!({"type": "MultiPolygon", "coordinates": [[ring(5)], [ring(5)]]})), (10, 7));
    }

    #[test]
    fn an_elevated_position_is_one_position() {
        pretty_assertions::assert_eq!(both(&json!({"type": "LineString", "coordinates": [[0, 0, 5.5], [1, 1, 6.25]]})), (2, 3));
    }

    /// Decimal ordinates reach a streaming visitor as the reader's number
    /// tokens; they are ordinates all the same.
    #[test]
    fn decimal_ordinates_count_no_values() {
        let text = r#"{"type": "Point", "coordinates": [14.550000000000000001, 46.05]}"#;
        let values = tally(usize::MAX);
        pretty_assertions::assert_eq!(text_geometry_census(text, limit(10), &values).expect("countable").get(), 1);
        pretty_assertions::assert_eq!(values.counted().get(), 2);
    }

    /// IETF RFC 7946 sections 5 and 6.1: a `bbox` and a foreign member are
    /// not part of the geometry, but each costs the values it holds.
    #[test]
    fn a_bbox_and_foreign_members_count_as_values() {
        let geometry = json!({"type": "Point", "coordinates": [0, 0], "bbox": [0, 0, 1, 1], "foreign": {"a": [1, 2, 3]}});
        // object, type, bbox and its four numbers, foreign, a and its three numbers
        pretty_assertions::assert_eq!(both(&geometry), (1, 12));
    }

    #[test]
    fn a_foreign_member_past_the_value_tally_is_refused() {
        let geometry = json!({"type": "Point", "coordinates": [0, 0], "foreign": (0..100).collect::<Vec<u32>>()});
        let refused = compact_geometry_census(&geometry, limit(10), &tally(50)).expect_err("a hundred values exceed fifty");
        assert!(
            matches!(refused, GeometryCensusError::OverLimit(GeometryOverLimit::Values { .. })),
            "{refused:?}"
        );
        let refused = text_geometry_census(&geometry.to_string(), limit(10), &tally(50)).expect_err("streamed alike");
        assert!(
            matches!(refused, GeometryCensusError::OverLimit(GeometryOverLimit::Values { .. })),
            "{refused:?}"
        );
    }

    /// Lists that hold no position cost a value each, so a geometry of
    /// nothing but empty lists is bounded too.
    #[test]
    fn empty_coordinate_lists_count_as_values() {
        let geometry = json!({"type": "MultiPolygon", "coordinates": [[], [], []]});
        pretty_assertions::assert_eq!(both(&geometry), (0, 6));
    }

    #[test]
    fn a_geometry_past_the_position_limit_is_refused_naming_the_limit() {
        let polygon = json!({"type": "Polygon", "coordinates": [ring(6)]});
        let refused = compact_geometry_census(&polygon, limit(5), &tally(100)).expect_err("six positions exceed five");
        assert!(
            matches!(refused, GeometryCensusError::OverLimit(GeometryOverLimit::Positions { limit: at }) if at == limit(5)),
            "{refused:?}"
        );
        pretty_assertions::assert_eq!(
            refused.to_string(),
            "the geometry holds more than the 5 positions this broker accepts in one geometry"
        );
        pretty_assertions::assert_eq!(
            compact_geometry_census(&polygon, limit(6), &tally(100)).expect("six positions are within six"),
            PositionCount::new(6)
        );
    }

    /// Clause 4.7.2: a geometry encoded as a JSON string is counted from its
    /// text, without building it.
    #[test]
    fn a_string_encoded_polygon_is_counted_without_building_it() {
        let polygon = json!({"type": "Polygon", "coordinates": [ring(5)]}).to_string();
        pretty_assertions::assert_eq!(text_geometry_census(&polygon, limit(5), &tally(100)).expect("within the limit").get(), 5);
        let refused = text_geometry_census(&polygon, limit(4), &tally(100)).expect_err("five positions exceed four");
        assert!(
            matches!(refused, GeometryCensusError::OverLimit(GeometryOverLimit::Positions { .. })),
            "{refused:?}"
        );
    }

    #[test]
    fn a_string_that_is_not_json_is_a_syntax_error() {
        let refused = text_geometry_census("{\"type\": \"Point\", ", limit(5), &tally(100)).expect_err("not JSON");
        assert!(matches!(refused, GeometryCensusError::Syntax(_)), "{refused:?}");
    }

    #[test]
    fn a_value_that_is_not_geojson_counts_its_values_and_no_positions() {
        for (value, values) in [
            (json!({"a": 1}), 2),
            (json!("text"), 1),
            (json!([1, 2]), 3),
            (json!({"coordinates": "none"}), 2),
            (json!({"coordinates": []}), 2),
        ] {
            pretty_assertions::assert_eq!(both(&value), (0, values), "{value}");
        }
    }

    #[test]
    fn the_position_count_of_a_built_geometry_matches_the_census() {
        for geometry in [
            json!({"type": "Point", "coordinates": [1.0, 2.0]}),
            json!({"type": "MultiPoint", "coordinates": [[1.0, 2.0], [3.0, 4.0, 5.0]]}),
            json!({"type": "LineString", "coordinates": [[0, 0], [1, 1], [2, 2]]}),
            json!({"type": "MultiLineString", "coordinates": [[[0, 0], [1, 1]], [[2, 2], [3, 3], [4, 4]]]}),
            json!({"type": "Polygon", "coordinates": [ring(7), ring(4)]}),
            json!({"type": "MultiPolygon", "coordinates": [[ring(5)], [ring(6), ring(4)]]}),
        ] {
            let built = Geometry::from_geojson_value(&geometry).expect("a valid geometry");
            pretty_assertions::assert_eq!(built.form().position_count().get(), both(&geometry).0, "{geometry}");
        }
    }

    /// A malformed geometry is counted by its shape; refusing it is the
    /// geometry admission's business. Members that hold values where an
    /// ordinate or a list belongs count those values.
    #[test]
    fn a_misnested_geometry_is_counted_by_the_arrays_it_holds() {
        let misnested = json!({"type": "Polygon", "coordinates": [[[0, 0], {"x": 1}, ["a", "b"], [[1, 2]]]]});
        // positions [0,0], ["a","b"], [1,2]; values: object, type, two lists,
        // the object and its member, the list holding [1,2]
        pretty_assertions::assert_eq!(both(&misnested), (3, 7));
    }

    /// Table 5.2.13-1: a geo-query's coordinates encoded as a string are
    /// counted from their text.
    #[test]
    fn string_encoded_coordinates_are_counted_without_building_them() {
        let limits = |positions: usize| GeometryCensusLimits {
            positions: limit(positions),
            values: JsonValueLimit::new(NonZeroUsize::new(100).expect("a positive test limit")),
        };
        let coordinates = Value::Array(vec![ring(5)]).to_string();
        pretty_assertions::assert_eq!(text_coordinates_census(&coordinates, limits(5)).expect("within the limit").get(), 5);
        let refused = text_coordinates_census(&coordinates, limits(4)).expect_err("five positions exceed four");
        assert!(
            matches!(refused, GeometryCensusError::OverLimit(GeometryOverLimit::Positions { .. })),
            "{refused:?}"
        );
        let empties = Value::Array(vec![json!([]); 200]).to_string();
        let refused = text_coordinates_census(&empties, limits(4)).expect_err("two hundred lists exceed a hundred values");
        assert!(
            matches!(refused, GeometryCensusError::OverLimit(GeometryOverLimit::Values { .. })),
            "{refused:?}"
        );
    }
}
