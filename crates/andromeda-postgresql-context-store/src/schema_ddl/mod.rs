//! Dialect DDL statements SeaQuery has no form for.
//!
//! `CREATE DATABASE` and `ALTER DATABASE ... SET` are not statements SeaQuery
//! models, and they carry no bound parameters a query builder would number. They
//! are spelled here, in the one module sanctioned to write dialect SQL by hand,
//! so no other module in the crate constructs SQL text. Every statement is a
//! whole administrative command, not a fragment interpolated into a larger
//! query, and each is documented with why SeaQuery cannot express it.

pub mod database;
pub mod database_settings;
