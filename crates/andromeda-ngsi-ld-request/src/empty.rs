//! The parameter set of an endpoint that accepts no query parameters.

use crate::query_parameter::{QueryParameter, QueryParameterSet};
use serde::Deserialize;

/// Query parameters for endpoints that accept none.
///
/// Every operation that defines no query parameter extracts it, so a
/// parameter the operation does not define is the clause 6.3.20
/// `InvalidRequest` rather than ignored. `deny_unknown_fields` is the point of
/// the type.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyParams {}

impl QueryParameterSet for EmptyParams {
    fn defines(_parameter: QueryParameter) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use crate::empty::EmptyParams;

    #[test]
    fn an_empty_query_is_accepted_and_any_parameter_is_not() {
        assert!(serde_urlencoded::from_str::<EmptyParams>("").is_ok());
        assert!(serde_urlencoded::from_str::<EmptyParams>("foo=bar").is_err());
    }
}
