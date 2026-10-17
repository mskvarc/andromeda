//! TimescaleDB procedural DDL SeaQuery has no form for.
//!
//! `CALL convert_to_columnstore(...)` is a procedure, and `ALTER TABLE ... SET
//! (timescaledb.enable_columnstore, ...)` a storage parameter list, neither of
//! which a query builder models: the hypertable is a fixed catalogue name, its
//! segment and order columns are fixed per hypertable, and a converted chunk is
//! named by `TimescaleDB`'s own catalogue. They are spelled here, in the one
//! module of this crate sanctioned to write dialect SQL by hand, so no other
//! module constructs SQL text.

pub mod timescale_policy;
