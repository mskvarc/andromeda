//! NGSI-LD query parsing, compilation, and SQL generation.
//!
//! This module implements the full query pipeline for the NGSI-LD context broker:
//!
//! 1. **Parsing** -- HTTP query parameters are deserialized into typed parameter structs
//!    (`EntityQueryParameters`, `TemporalQueryParameters`, `CsrQueryParameters`).
//! 2. **Compilation** -- Raw parameters are validated, context-expanded, and compiled into
//!    intermediate representations (`CompiledEntityQuery`, `CompiledTemporalQuery`,
//!    `CompiledCsrQuery`) that are decoupled from HTTP concerns.
//! 3. **SQL generation** -- Compiled queries are translated into parameterized SQL with
//!    bind values for execution against the persistence layer.
//!
//! The `q` sub-module handles the NGSI-LD query language (the `q` parameter) including
//! tokenization, parsing, AST representation, evaluation, and SQL serialization.

/// Push a SQL condition with a bind parameter, incrementing the parameter index.
///
/// This replaces the repeated three-line pattern of pushing a formatted condition,
/// pushing a bind value, and incrementing the parameter index.
macro_rules! bind_condition {
    ($conditions:expr, $binds:expr, $idx:expr, $fmt:expr, $bind_val:expr) => {{
        $conditions.push(format!($fmt, *$idx));
        $binds.push($bind_val);
        *$idx += 1;
    }};
}

/// Generate a [`params::SharedQueryParams`] implementation for a query parameter struct.
///
/// Maps struct fields to the trait's raw accessor methods. Use `field_name` to
/// map a field via `self.field_name.as_deref()` (for `Option<String>`) or
/// `self.field_name` (for `Option<u32>`). Use `None` for parameters the struct
/// does not support.
macro_rules! impl_shared_query_params {
    ($ty:ty { id: $id:ident, attrs: $attrs:ident,
              pick: $($pick:ident)?, omit: $($omit:ident)?,
              dataset_id: $($ds:ident)?, limit: $lim:ident, offset: $off:ident }) => {
        impl crate::query::params::SharedQueryParams for $ty {
            fn raw_id(&self) -> Option<&str> { self.$id.as_deref() }
            fn raw_attrs(&self) -> Option<&str> { self.$attrs.as_deref() }
            fn raw_pick(&self) -> Option<&str> { impl_shared_query_params!(@opt self $($pick)?) }
            fn raw_omit(&self) -> Option<&str> { impl_shared_query_params!(@opt self $($omit)?) }
            fn raw_dataset_id(&self) -> Option<&str> { impl_shared_query_params!(@opt self $($ds)?) }
            fn raw_limit(&self) -> Option<u32> { self.$lim }
            fn raw_offset(&self) -> Option<u32> { self.$off }
        }
    };
    (@opt $self:ident) => { None };
    (@opt $self:ident $field:ident) => { $self.$field.as_deref() };
}

/// Deserialize a query string into a query parameter struct for testing.
///
/// Wraps `serde_urlencoded::from_str` for use in test helper functions.
#[cfg(test)]
macro_rules! params_from_query {
    ($query:expr, $ty:ty) => {
        serde_urlencoded::from_str::<$ty>($query).unwrap()
    };
}

pub mod compiled;
pub mod csr;
pub mod entity;
pub mod error;
pub mod params;
pub mod q;
pub mod temporal;

pub mod compile;
pub mod csr_sql;
pub mod geo;
pub mod sql;
pub mod temporal_sql;
