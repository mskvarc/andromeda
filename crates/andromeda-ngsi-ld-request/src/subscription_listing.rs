//! The parameters of the subscription collections: `GET /subscriptions` and
//! `GET /csourceSubscriptions` (clauses 5.8.4 and 5.11.5).

use crate::{
    error::ParameterError,
    options::SysAttrsOptions,
    pagination::{
        page_size::{BROKER_PAGE_SIZE_CAP, PageSizeBounds},
        parameters::PageParameters,
    },
    query_parameter::{QueryParameter, QueryParameterSet},
    query_parameter_set,
    representation_parameter::{WrittenValue, supported_option},
};
use andromeda_ngsi_ld_query::count_policy::CountPolicy;
use serde::Deserialize;

/// The page size of the subscription collections: 100 when the request names
/// no `limit`, and at most the broker's cap, since a tenant can hold any
/// number of subscriptions.
pub const SUBSCRIPTION_PAGE_SIZE: PageSizeBounds = PageSizeBounds {
    default: 100,
    cap: BROKER_PAGE_SIZE_CAP,
};

/// Query parameters for listing subscriptions of either kind.
///
/// The two collections define the same parameters (Tables 6.10.3.2-1 and
/// 6.12.3.2-1), so one parameter set reads both. Clause 4.12 makes both
/// paginated operations, so clause 6.3.13 adds `count` beside the page.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// The page asked for (clauses 5.5.9 and 6.3.10 `limit`, `offset` and
    /// `cursor`), decided by the broker's pagination policy.
    #[serde(flatten)]
    pub page: PageParameters,
    /// Whether the response reports the total number of subscriptions
    /// (clause 6.3.13 `count`).
    #[serde(default)]
    pub count: CountPolicy,
    /// The system-attribute selection (clause 6.3.11 `options`).
    pub options: Option<WrittenValue<SysAttrsOptions>>,
}

query_parameter_set!(ListParams { Count, Options } + PageParameters);

impl ListParams {
    /// The system-attribute selection the `options` parameter made.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::OptionsInvalid`] for a value clause 6.3.11
    /// does not define, which clause 6.3.20 answers with `InvalidRequest`.
    pub fn parse_options(&self) -> Result<SysAttrsOptions, ParameterError> {
        Ok(supported_option(self.options.as_ref())?.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::ParameterError,
        pagination::page_size::PageSizeExceeded,
        subscription_listing::{ListParams, SUBSCRIPTION_PAGE_SIZE},
    };
    use andromeda_ngsi_ld_query::count_policy::CountPolicy;
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use pretty_assertions::assert_eq;

    /// The subscription collections answer with 100 by default, serve 1000
    /// and refuse more.
    #[test]
    fn subscription_page_size_is_one_hundred_capped_at_one_thousand() {
        assert_eq!(SUBSCRIPTION_PAGE_SIZE.limit(None), Ok(100));
        assert_eq!(SUBSCRIPTION_PAGE_SIZE.limit(Some(1000)), Ok(1000));
        assert_eq!(SUBSCRIPTION_PAGE_SIZE.limit(Some(1001)), Err(PageSizeExceeded { requested: 1001, cap: 1000 }));
    }

    /// The page, the count and the options (clauses 6.3.10, 6.3.13 and 6.3.11)
    /// are read side by side.
    #[test]
    fn the_page_the_count_and_the_options_are_read_together() {
        let params: ListParams = serde_urlencoded::from_str("limit=3&offset=6&count=true&options=sysAttrs").expect("parses");
        assert_eq!(params.page.limit, Some(3));
        assert_eq!(params.page.offset, Some(6));
        assert_eq!(params.count, CountPolicy::Emit);
        assert_eq!(params.parse_options().expect("valid options").sys_attrs, SysAttrsInclusion::Include);
    }

    /// Without `count` the listing reports no total (Table 6.3.13-1).
    #[test]
    fn an_absent_count_asks_for_no_total() {
        let params: ListParams = serde_urlencoded::from_str("limit=3").expect("parses");
        assert_eq!(params.count, CountPolicy::Omit);
    }

    /// Table 6.3.11-1 gives the listings' `options` only `sysAttrs`; the count
    /// is the clause 6.3.13 parameter of its own.
    #[test]
    fn a_count_through_options_is_refused() {
        let params: ListParams = serde_urlencoded::from_str("options=count").expect("parses");
        assert!(matches!(params.parse_options(), Err(ParameterError::OptionsInvalid(_))));
    }

    /// A parameter the collections do not define is refused as unknown.
    #[test]
    fn an_undefined_parameter_is_unknown() {
        let message = serde_urlencoded::from_str::<ListParams>("page=2").expect_err("rejected").to_string();
        assert!(message.contains("unknown field"), "{message}");
    }
}
