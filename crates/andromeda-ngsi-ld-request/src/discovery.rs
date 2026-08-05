//! The parameters of the `/types` and `/attributes` list endpoints.

use crate::case_insensitive_bool::deserialize_option_bool_case_insensitive;
use serde::Deserialize;

/// Query parameters shared by the two discovery list endpoints (clause 5.7.6
/// and 5.7.9).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryListParams {
    /// Whether to answer with the detailed representation.
    #[serde(default)]
    pub details: bool,
    /// Whether the listing is confined to this broker.
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub local: Option<bool>,
}

#[cfg(test)]
mod tests {
    use crate::discovery::DiscoveryListParams;

    #[test]
    fn both_parameters_default_to_absent() {
        let params: DiscoveryListParams = serde_urlencoded::from_str("").expect("parses");
        assert!(!params.details);
        pretty_assertions::assert_eq!(params.local, None);
    }

    #[test]
    fn both_parameters_are_read_when_present() {
        let params: DiscoveryListParams = serde_urlencoded::from_str("details=true&local=false").expect("parses");
        assert!(params.details);
        pretty_assertions::assert_eq!(params.local, Some(false));
    }
}
