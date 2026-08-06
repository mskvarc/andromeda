//! The parameters of the NGSI-LD query endpoints, and what they have in common.
//!
//! Entity, temporal and registration queries each define their own parameter
//! set, but clause 6.3.x gives all three the same selection, projection and
//! pagination parameters with the same spellings. [`SharedQueryParams`] is that
//! overlap, declared once with its parsing, so a new query endpoint inherits
//! the behaviour instead of restating it.

pub mod csr;
pub mod entity;
pub mod entity_selector;
pub mod temporal;

use andromeda_ngsi_ld_lexicon::uri::Uri;

/// Generate a trait method that splits a raw comma-separated parameter.
///
/// The generated method answers with an empty vector when the parameter is
/// absent, because "not narrowed" and "narrowed to nothing" are the same thing
/// for every parameter this covers.
macro_rules! csv_parser {
    ($method:ident, $raw:ident, $doc:expr) => {
        #[doc = $doc]
        ///
        /// Returns an empty vector if the parameter is absent.
        fn $method(&self) -> Vec<String> {
            self.$raw()
                .map(|raw| raw.split(',').map(|value| value.trim().to_string()).collect())
                .unwrap_or_default()
        }
    };
}

/// The selection, projection and pagination parameters every query endpoint has.
///
/// Implementors supply the raw accessors; the parsing is defined once here.
pub trait SharedQueryParams {
    /// The raw `id` parameter, if present.
    fn raw_id(&self) -> Option<&str>;

    /// The raw `attrs` parameter, if present.
    fn raw_attrs(&self) -> Option<&str>;

    /// The raw `pick` parameter, if present.
    fn raw_pick(&self) -> Option<&str>;

    /// The raw `omit` parameter, if present.
    fn raw_omit(&self) -> Option<&str>;

    /// The raw `datasetId` parameter, if present.
    fn raw_dataset_id(&self) -> Option<&str>;

    /// The raw `limit` parameter, if present.
    fn raw_limit(&self) -> Option<u32>;

    /// The raw `offset` parameter, if present.
    fn raw_offset(&self) -> Option<u32>;

    /// The `id` parameter as a list of entity identifiers.
    ///
    /// `None` when the parameter is absent, which is not the same as an empty
    /// list: absent means "any entity", empty means the request named none.
    fn parse_ids(&self) -> Option<Vec<String>> {
        self.raw_id()
            .map(|raw| raw.split(',').map(|identifier| identifier.trim().to_string()).collect())
    }

    csv_parser!(
        parse_attrs,
        raw_attrs,
        "Parses the `attrs` parameter as a comma-separated list of attribute names."
    );
    csv_parser!(
        parse_pick,
        raw_pick,
        "Parses the `pick` parameter as a comma-separated list of attribute names to include (clause 6.3.15)."
    );
    csv_parser!(
        parse_omit,
        raw_omit,
        "Parses the `omit` parameter as a comma-separated list of attribute names to exclude (clause 6.3.15)."
    );
    csv_parser!(
        parse_dataset_ids,
        raw_dataset_id,
        "Parses the `datasetId` parameter as a comma-separated list of dataset identifiers."
    );

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

/// Whether an entity identifier is a well-formed URI (clause 4.5.1).
pub(crate) fn is_valid_uri(identifier: &str) -> bool {
    identifier.parse::<Uri>().is_ok()
}

/// Implement [`SharedQueryParams`] for a query parameter struct.
///
/// Maps struct fields onto the trait's raw accessors. A parameter the struct
/// does not define is written as an empty position, which yields `None`.
#[macro_export]
macro_rules! impl_shared_query_params {
    ($ty:ty { id: $id:ident, attrs: $attrs:ident,
              /// Members the response is narrowed to (clause 6.3.15 `pick`).
              pick: $($pick:ident)?, omit: $($omit:ident)?,
              /// Dataset instances to select, comma-separated (`datasetId`).
              dataset_id: $($ds:ident)?, limit: $lim:ident, offset: $off:ident }) => {
        impl $crate::query::SharedQueryParams for $ty {
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
    use crate::query::is_valid_uri;

    #[test]
    fn a_urn_and_an_http_url_are_both_valid_identifiers() {
        assert!(is_valid_uri("urn:ngsi-ld:Vehicle:1"));
        assert!(is_valid_uri("https://example.com/entities/1"));
    }

    #[test]
    fn a_bare_word_is_not_an_identifier() {
        assert!(!is_valid_uri("thisisaninvaliduri"));
        assert!(!is_valid_uri(""));
    }
}
