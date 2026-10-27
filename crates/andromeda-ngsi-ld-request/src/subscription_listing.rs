//! The parameters of the subscription collections: `GET /subscriptions` and
//! `GET /csourceSubscriptions` (clauses 5.8.4 and 5.11.4).

use crate::{
    error::ParameterError,
    options::ListOptions,
    pagination::{
        page_size::{BROKER_PAGE_SIZE_CAP, PageSizeBounds},
        parameters::PageParameters,
    },
    query_parameter::{QueryParameter, QueryParameterSet},
    query_parameter_set,
    representation_parameter::{WrittenValue, supported_option},
};
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
/// 6.13.3.2-1), so one parameter set reads both.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListParams {
    /// The page asked for (clauses 5.5.9 and 6.3.10 `limit`, `offset` and
    /// `cursor`), decided by the broker's pagination policy.
    #[serde(flatten)]
    pub page: PageParameters,
    /// Typed count and system-attribute response options (`options`).
    pub options: Option<WrittenValue<ListOptions>>,
}

query_parameter_set!(ListParams { Options } + PageParameters);

impl ListParams {
    /// The count and system-attribute selection the `options` parameter made.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::OptionsInvalid`] for a value clauses 6.3.11
    /// and 6.3.13 do not define, which clause 6.3.20 answers with
    /// `InvalidRequest`.
    pub fn list_options(&self) -> Result<ListOptions, ParameterError> {
        Ok(supported_option(self.options.as_ref())?.unwrap_or_default())
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        pagination::page_size::PageSizeExceeded,
        subscription_listing::{ListParams, SUBSCRIPTION_PAGE_SIZE},
    };
    use andromeda_ngsi_ld_query::count_policy::CountPolicy;
    use pretty_assertions::assert_eq;

    /// The subscription collections answer with 100 by default, serve 1000
    /// and refuse more.
    #[test]
    fn subscription_page_size_is_one_hundred_capped_at_one_thousand() {
        assert_eq!(SUBSCRIPTION_PAGE_SIZE.limit(None), Ok(100));
        assert_eq!(SUBSCRIPTION_PAGE_SIZE.limit(Some(1000)), Ok(1000));
        assert_eq!(SUBSCRIPTION_PAGE_SIZE.limit(Some(1001)), Err(PageSizeExceeded { requested: 1001, cap: 1000 }));
    }

    /// The page and the options are read side by side.
    #[test]
    fn the_page_and_the_options_are_read_together() {
        let params: ListParams = serde_urlencoded::from_str("limit=3&offset=6&options=count").expect("parses");
        assert_eq!(params.page.limit, Some(3));
        assert_eq!(params.page.offset, Some(6));
        assert_eq!(params.list_options().expect("valid options").count, CountPolicy::Emit);
    }

    /// A parameter the collections do not define is refused as unknown.
    #[test]
    fn an_undefined_parameter_is_unknown() {
        let message = serde_urlencoded::from_str::<ListParams>("page=2").expect_err("rejected").to_string();
        assert!(message.contains("unknown field"), "{message}");
    }
}
