//! The parameters of the NGSI-LD query endpoints, and what they have in common.
//!
//! Entity, temporal and registration queries each define their own parameter
//! set, but clause 6.3.x gives all three the same selection, projection and
//! pagination parameters with the same spellings. [`SharedQueryParams`] is that
//! overlap, declared once with typed accessors so a new query endpoint inherits
//! the behaviour instead of restating it.

pub mod csr;
pub mod deserialization;
pub mod entity;
pub mod entity_selector;
pub mod temporal;

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{dataset::DatasetSelector, projection::grammar::RawProjectionTerm};

/// The selection, projection and pagination parameters every query endpoint has.
///
/// Implementors supply typed request values.
pub trait SharedQueryParams {
    /// The `attrs` parameter, if present.
    fn attrs(&self) -> Option<&[AttributeName]>;

    /// The `pick` parameter, if present.
    fn pick(&self) -> Option<&[RawProjectionTerm]>;

    /// The `omit` parameter, if present.
    fn omit(&self) -> Option<&[RawProjectionTerm]>;

    /// The `datasetId` URI/default selectors, if present.
    fn dataset_ids(&self) -> Option<&[DatasetSelector]>;

    /// The raw `limit` parameter, if present.
    fn raw_limit(&self) -> Option<u32>;

    /// The raw `offset` parameter, if present.
    fn raw_offset(&self) -> Option<u32>;

    /// The pagination limit, defaulted and capped.
    ///
    /// Defaults to 20 when absent. The cap of 1000 is what stops a single
    /// request from asking for an unbounded result set.
    fn effective_limit(&self) -> u32 {
        self.raw_limit().unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT)
    }

    /// The pagination limit with no upper cap.
    ///
    /// Defaults to `u32::MAX` when absent. For callers that bound the result
    /// set themselves and would otherwise be silently truncated at the cap.
    fn effective_limit_uncapped(&self) -> u32 {
        self.raw_limit().unwrap_or(u32::MAX)
    }

    /// The pagination offset, defaulting to 0.
    fn effective_offset(&self) -> u32 {
        self.raw_offset().unwrap_or(0)
    }
}

/// The page size a query returns when it does not ask for one.
const DEFAULT_LIMIT: u32 = 20;

/// The largest page size a query may ask for.
const MAX_LIMIT: u32 = 1000;

/// Implement [`SharedQueryParams`] for a query parameter struct.
///
/// Maps struct fields onto the trait's raw accessors. A parameter the struct
/// does not define is written as an empty position, which yields `None`.
#[macro_export]
macro_rules! impl_shared_query_params {
    ($ty:ty { id: $id:ident, attrs: $attrs:ident,
              /// Members the response is narrowed to (clause 4.21 `pick`).
              pick: $($pick:ident)?, omit: $($omit:ident)?,
              /// Dataset instances to select, comma-separated (`datasetId`).
              dataset_id: $($ds:ident)?, limit: $lim:ident, offset: $off:ident }) => {
        impl SharedQueryParams for $ty {
            fn attrs(&self) -> Option<&[AttributeName]> { self.$attrs.as_deref() }
            fn pick(&self) -> Option<&[RawProjectionTerm]> { impl_shared_query_params!(@opt self $($pick)?) }
            fn omit(&self) -> Option<&[RawProjectionTerm]> { impl_shared_query_params!(@opt self $($omit)?) }
            fn dataset_ids(&self) -> Option<&[DatasetSelector]> { impl_shared_query_params!(@opt self $($ds)?) }
            fn raw_limit(&self) -> Option<u32> { self.$lim }
            fn raw_offset(&self) -> Option<u32> { self.$off }
        }
    };
    (@opt $self:ident) => { None };
    (@opt $self:ident $field:ident) => { $self.$field.as_deref() };
}

/// Deserialise a query string into a parameter struct, for tests.
#[cfg(test)]
#[macro_export]
macro_rules! params_from_query {
    ($query:expr, $ty:ty) => {
        serde_urlencoded::from_str::<$ty>($query).expect("query parses")
    };
}

#[cfg(test)]
mod tests {}
