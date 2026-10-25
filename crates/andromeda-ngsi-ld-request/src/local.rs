//! The `local` parameter, for endpoints that accept nothing else.

use serde::Deserialize;

/// Query parameters for endpoints that accept only `local`.
///
/// Shared by entity creation, deletion, merge and update, the batch creation,
/// deletion and merge, and the type and attribute retrieval endpoints: clause
/// 6.3.18 gives every operation on `/entities/`, `/types/`, `/attributes/` and
/// `/entityOperations/` the parameter. `local=true` confines an operation to
/// this broker instead of forwarding it (clause 5.5.13).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalOnlyParams {
    /// Whether the operation is confined to this broker.
    pub local: Option<bool>,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::local::LocalOnlyParams;

    #[test]
    fn the_flag_is_read_and_defaults_to_absent() {
        pretty_assertions::assert_eq!(serde_urlencoded::from_str::<LocalOnlyParams>("").expect("parses").local, None);
        pretty_assertions::assert_eq!(serde_urlencoded::from_str::<LocalOnlyParams>("local=true").expect("parses").local, Some(true));
        pretty_assertions::assert_eq!(serde_urlencoded::from_str::<LocalOnlyParams>("local=false").expect("parses").local, Some(false));
    }

    #[test]
    fn a_non_boolean_or_an_unknown_parameter_is_rejected() {
        assert!(serde_urlencoded::from_str::<LocalOnlyParams>("local=yes").is_err());
        assert!(serde_urlencoded::from_str::<LocalOnlyParams>("unknown=x").is_err());
    }

    /// Clause 4.9 binds `true` and `false` to the RFC 8259 literals, which are
    /// lowercase.
    #[test]
    fn a_boolean_outside_lowercase_is_rejected() {
        for query in ["local=TRUE", "local=True", "local=FALSE", "local=False"] {
            assert!(serde_urlencoded::from_str::<LocalOnlyParams>(query).is_err(), "{query}");
        }
    }
}
