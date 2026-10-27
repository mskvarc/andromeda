//! The parameter set of an operation whose only URL parameter is the clause
//! 6.3.11 `options`.
//!
//! Retrieve Subscription and Retrieve Context Source Registration Subscription
//! define no URL parameter of their own (Tables 6.11.3.1-1 and 6.13.3.1-1), and
//! clause 6.3.11 gives every GET under `/subscriptions/` and
//! `/csourceSubscriptions/` the `options` parameter. Anything else, the
//! listing's paging and its `count` included, is a parameter the operation
//! does not define: the clause 6.3.20 `InvalidRequest`.

use crate::{
    error::ParameterError,
    options::SysAttrsOptions,
    query_parameter::{QueryParameter, QueryParameterSet},
    query_parameter_set,
    representation_parameter::{WrittenValue, supported_option},
};
use serde::Deserialize;

/// Query parameters of an operation that takes `options=sysAttrs` alone.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SysAttrsParams {
    /// The system-attribute selection (clause 6.3.11 `options`).
    pub options: Option<WrittenValue<SysAttrsOptions>>,
}

query_parameter_set!(SysAttrsParams { Options });

impl SysAttrsParams {
    /// The system-attribute selection the `options` parameter made.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::OptionsInvalid`] for a value clause 6.3.11
    /// does not define (clause 6.3.20).
    pub fn parse_options(&self) -> Result<SysAttrsOptions, ParameterError> {
        Ok(supported_option(self.options.as_ref())?.unwrap_or_default())
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::sys_attrs_parameters::SysAttrsParams;
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use pretty_assertions::assert_eq;

    #[test]
    fn options_sys_attrs_is_read_and_its_absence_strips() {
        let params: SysAttrsParams = serde_urlencoded::from_str("options=sysAttrs").expect("parses");
        assert_eq!(params.parse_options().expect("a supported options value").sys_attrs, SysAttrsInclusion::Include);
        let params: SysAttrsParams = serde_urlencoded::from_str("").expect("parses");
        assert_eq!(params.parse_options().expect("no options value").sys_attrs, SysAttrsInclusion::Strip);
    }

    /// Tables 6.11.3.1-1 and 6.13.3.1-1 define no paging for one resource.
    #[test]
    fn the_listing_paging_is_unknown() {
        for query in ["limit=3", "offset=1", "cursor=abc"] {
            let message = serde_urlencoded::from_str::<SysAttrsParams>(query).expect_err("refused").to_string();
            assert!(message.contains("unknown field"), "{query}: {message}");
        }
    }

    /// `count` belongs to a listing (clause 6.3.13), not to one resource.
    #[test]
    fn options_count_is_refused() {
        let params: SysAttrsParams = serde_urlencoded::from_str("options=count").expect("parses");
        assert!(params.parse_options().is_err());
    }
}
