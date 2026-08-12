//! Verification suite for the query access paths of the redesigned schema.
//!
//! Covers the database-schema-redesign verification-matrix rows "Typed q
//! predicates", "Arbitrary GeoProperty", "Entity access-path plans",
//! "Identifier collation", "Object GIN operator class", "Extended statistics",
//! "Physical column layout", and "Semantic-time plans".
//!
//! The measurement-shaped rows are executed as SQL checks with a pass/fail
//! assertion — `EXPLAIN (ANALYZE, BUFFERS)`, `pg_attribute` offsets,
//! `pg_statistic_ext_data` — never as a benchmark whose result a reader has to
//! interpret.
//!
//! Naming convention: `<row_topic>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

mod column_layout;
mod entity_plans;
mod extended_statistics;
mod geoproperty;
mod identifier_collation;
mod object_predicates;
mod seeded_corpus;
mod seeded_history;
mod semantic_time;
mod typed_q;
