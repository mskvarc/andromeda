//! When a request may name an `entityMapLifetime`.
//!
//! Tables 6.4.3.2-1, 6.5.3.1-1 and 6.18.3.2-1, and the Query data type of
//! clause 5.2.23, make the lifetime "Only applicable if entityMap is set to
//! true": it is how long the map the request asks for lives, so without that
//! ask there is no map for it to apply to and the parameter is incompatible
//! with the request (clause 6.3.20). The Entity Map creations always create a
//! map and take a lifetime without `entityMap`.

use crate::error::ParameterError;
use andromeda_ngsi_ld_lexicon::duration::Iso8601Duration;

/// Refuse a lifetime the request asks for no map to apply to.
///
/// # Errors
///
/// Returns [`ParameterError::EntityMapLifetimeWithoutEntityMap`] when
/// `lifetime` is set and `entity_map` is not `true`.
pub const fn require_entity_map_for_lifetime(entity_map: Option<bool>, lifetime: Option<&Iso8601Duration>) -> Result<(), ParameterError> {
    match (entity_map, lifetime) {
        (Some(false) | None, Some(_)) => Err(ParameterError::EntityMapLifetimeWithoutEntityMap),
        (Some(true), _) | (_, None) => Ok(()),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{entity_map_lifetime::require_entity_map_for_lifetime, error::ParameterError};
    use andromeda_ngsi_ld_lexicon::duration::Iso8601Duration;

    fn lifetime() -> Iso8601Duration {
        "PT1H".parse().expect("a valid duration")
    }

    #[test]
    fn a_lifetime_without_entity_map_true_is_refused() {
        let lifetime = lifetime();
        for entity_map in [None, Some(false)] {
            let result = require_entity_map_for_lifetime(entity_map, Some(&lifetime));
            assert!(
                matches!(result, Err(ParameterError::EntityMapLifetimeWithoutEntityMap)),
                "{entity_map:?}: {result:?}"
            );
        }
    }

    #[test]
    fn a_lifetime_beside_entity_map_true_and_a_flag_alone_are_admitted() {
        let lifetime = lifetime();
        assert!(require_entity_map_for_lifetime(Some(true), Some(&lifetime)).is_ok());
        for entity_map in [None, Some(false), Some(true)] {
            assert!(require_entity_map_for_lifetime(entity_map, None).is_ok(), "{entity_map:?}");
        }
    }
}
