//! Verification suite for the query access paths of the schema.
//!
//! Covers typed `q` predicates, queries on an arbitrary `GeoProperty`, entity
//! access-path plans, identifier collation, the object GIN operator class,
//! extended statistics, the physical column layout, semantic-time plans,
//! temporal access-path plans, write-record release plans, registration
//! routing plans and Entity Map read plans.
//!
//! The measurement-shaped properties run as SQL checks with a pass/fail
//! assertion — `EXPLAIN (ANALYZE, BUFFERS)`, `pg_attribute` offsets,
//! `pg_statistic_ext_data` — never as a benchmark whose result a reader has to
//! interpret.
//!
//! Naming convention: `<property>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod column_layout;
mod entity_map_reads;
mod entity_plans;
mod extended_statistics;
mod geoproperty;
mod identifier_collation;
mod object_predicates;
mod registration_routing;
mod seeded_corpus;
mod seeded_history;
mod semantic_time;
mod temporal_plans;
mod temporal_q_plans;
mod typed_q;
mod write_record_release;
