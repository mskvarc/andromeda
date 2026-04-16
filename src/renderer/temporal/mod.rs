//! Temporal response rendering (NGSI-LD clause 4.5.5, 6.3.10).
//!
//! Each renderer lives in its own submodule:
//!
//! - [`entity::TemporalEntityRenderer`] -- assembles a temporal entity from
//!   database rows.
//! - [`aggregation::TemporalAggregationRenderer`] -- extracts aggregation
//!   method values (avg, min, max, ...) from bucketed rows.
//! - [`values::TemporalValuesRenderer`] -- transforms a compacted temporal
//!   entity into the simplified `temporalValues` representation.
//! - [`array_normalizer::TemporalArrayNormalizer`] -- re-wraps single-object
//!   attributes as arrays after JSON-LD compaction may have collapsed them.
//! - [`simplify::TemporalSimplifyRenderer`] -- simplifies a single or a
//!   collection of temporal entity values into key-values form.

pub mod aggregation;
pub mod array_normalizer;
pub mod entity;
pub mod simplify;
pub mod values;

mod common;
