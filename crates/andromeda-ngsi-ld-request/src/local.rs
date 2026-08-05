//! The `local` parameter, for endpoints that accept nothing else.

use crate::case_insensitive_bool::deserialize_option_bool_case_insensitive;
use serde::Deserialize;

/// Query parameters for endpoints that accept only `local`.
///
/// Shared by entity creation, deletion, merge and update, and by the type and
/// attribute retrieval endpoints. `local=true` confines an operation to this
/// broker instead of forwarding it (clause 5.10).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalOnlyParams {
    /// Whether the operation is confined to this broker.
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub local: Option<bool>,
}

#[cfg(test)]
mod tests {
    use crate::local::LocalOnlyParams;

    #[test]
    fn the_flag_parses_in_any_case_and_defaults_to_absent() {
        pretty_assertions::assert_eq!(serde_urlencoded::from_str::<LocalOnlyParams>("").expect("parses").local, None);
        pretty_assertions::assert_eq!(serde_urlencoded::from_str::<LocalOnlyParams>("local=TRUE").expect("parses").local, Some(true));
        pretty_assertions::assert_eq!(serde_urlencoded::from_str::<LocalOnlyParams>("local=false").expect("parses").local, Some(false));
    }

    #[test]
    fn a_non_boolean_or_an_unknown_parameter_is_rejected() {
        assert!(serde_urlencoded::from_str::<LocalOnlyParams>("local=yes").is_err());
        assert!(serde_urlencoded::from_str::<LocalOnlyParams>("unknown=x").is_err());
    }
}
