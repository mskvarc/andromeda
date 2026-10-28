//! The parameters of the NGSI-LD query endpoints, and what they have in common.
//!
//! Entity, temporal and registration queries each define their own parameter
//! set, but clause 6.3.x gives all three the same selection and projection
//! parameters with the same spellings. [`SharedQueryParams`] is that overlap,
//! declared once with typed accessors so a new query endpoint inherits the
//! behaviour instead of restating it. Their pagination parameters are the
//! [`crate::pagination::parameters::PageParameters`] every paged collection
//! flattens, bounded by [`QUERY_PAGE_SIZE`].

pub mod csr;
pub mod deserialization;
pub mod entity;
pub mod entity_selector;
pub mod parameter_error;
pub mod temporal;

use crate::pagination::page_size::{BROKER_PAGE_SIZE_CAP, PageSizeBounds};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{dataset::DatasetSelector, projection::grammar::RawProjectionTerm};

/// The selection and projection parameters every query endpoint has.
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
}

/// The page size of the three query endpoints: the broker's cap of one
/// thousand when the request names no `limit`, and at most that cap when it
/// names one (clause 5.5.6 leaves both to the broker).
pub const QUERY_PAGE_SIZE: PageSizeBounds = PageSizeBounds {
    default: 1000,
    cap: BROKER_PAGE_SIZE_CAP,
};

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
              dataset_id: $($ds:ident)? }) => {
        impl SharedQueryParams for $ty {
            fn attrs(&self) -> Option<&[AttributeName]> { self.$attrs.as_deref() }
            fn pick(&self) -> Option<&[RawProjectionTerm]> { impl_shared_query_params!(@opt self $($pick)?) }
            fn omit(&self) -> Option<&[RawProjectionTerm]> { impl_shared_query_params!(@opt self $($omit)?) }
            fn dataset_ids(&self) -> Option<&[DatasetSelector]> { impl_shared_query_params!(@opt self $($ds)?) }
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
mod tests {
    use crate::{pagination::page_size::PageSizeExceeded, query::QUERY_PAGE_SIZE};
    use pretty_assertions::assert_eq;

    /// The query endpoints answer with 1000 by default, serve a smaller page
    /// when asked and refuse more.
    #[test]
    fn query_page_size_defaults_to_its_cap_of_one_thousand() {
        assert_eq!(QUERY_PAGE_SIZE.limit(None), Ok(1000));
        assert_eq!(QUERY_PAGE_SIZE.limit(Some(20)), Ok(20));
        assert_eq!(QUERY_PAGE_SIZE.limit(Some(1000)), Ok(1000));
        assert_eq!(QUERY_PAGE_SIZE.limit(Some(5000)), Err(PageSizeExceeded { requested: 5000, cap: 1000 }));
    }
}
