//! TimescaleDB procedural DDL SeaQuery has no form for.
//!
//! `CALL add_columnstore_policy(...)` and its removal are procedures, not
//! statements a query builder models, and they carry no bound parameters a
//! builder would number: the hypertable is one of two fixed catalogue names
//! and the lag is a validated day count from the broker's own configuration.
//! They are spelled here, in the one module of this crate sanctioned to write
//! dialect SQL by hand, so no other module constructs SQL text.

pub mod timescale_policy;
