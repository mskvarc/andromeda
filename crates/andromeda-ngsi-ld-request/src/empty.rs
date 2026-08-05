//! The parameter set of an endpoint that accepts no query parameters.

use serde::Deserialize;

/// Query parameters for endpoints that accept none.
///
/// Used by subscription creation, context source subscription creation and
/// temporal entity creation. `deny_unknown_fields` is the point of the type:
/// a parameter the endpoint does not define is rejected rather than ignored.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyParams {}

#[cfg(test)]
mod tests {
    use crate::empty::EmptyParams;

    #[test]
    fn an_empty_query_is_accepted_and_any_parameter_is_not() {
        assert!(serde_urlencoded::from_str::<EmptyParams>("").is_ok());
        assert!(serde_urlencoded::from_str::<EmptyParams>("foo=bar").is_err());
    }
}
