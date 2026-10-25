//! Resolving the broker's pagination policy from the two sections that shape
//! it: `[pagination]`, which picks the default mode, and `[extensions]`, which
//! says whether opaque cursors are available at all.

use andromeda_extensions::config::ExtensionsConfig;
use andromeda_http_stack::config::feature_mode::HttpFeatureMode;
use andromeda_ngsi_ld_request::pagination::{
    config::PaginationConfig,
    cursor_availability::CursorAvailability,
    policy::{PaginationPolicy, PaginationPolicyError},
};

/// Whether cursor pagination is available, and who turned it off if not.
///
/// The conformance override forces every extension off whatever its field
/// says, so it is consulted first.
pub(crate) const fn cursor_availability(extensions: &ExtensionsConfig) -> CursorAvailability {
    match (extensions.conformance_mode, extensions.cursor_pagination) {
        (HttpFeatureMode::Enabled, HttpFeatureMode::Enabled | HttpFeatureMode::Disabled) => CursorAvailability::ForcedOffByConformance,
        (HttpFeatureMode::Disabled, HttpFeatureMode::Disabled) => CursorAvailability::DisabledByOperator,
        (HttpFeatureMode::Disabled, HttpFeatureMode::Enabled) => CursorAvailability::Enabled,
    }
}

/// The pagination policy `extensions` and `pagination` resolve to.
///
/// # Errors
///
/// Returns [`PaginationPolicyError::CursorDefaultWithoutCursorPagination`] when
/// the operator disabled cursor pagination and left cursors the default mode.
pub(crate) const fn resolve_pagination(extensions: &ExtensionsConfig, pagination: PaginationConfig) -> Result<PaginationPolicy, PaginationPolicyError> {
    PaginationPolicy::resolve(pagination.default_mode, cursor_availability(extensions))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::config::{error::ConfigurationError, pagination::resolve_pagination, runtime::RuntimeConfig, validation::validate_config};
    use andromeda_http_stack::config::feature_mode::HttpFeatureMode;
    use andromeda_ngsi_ld_request::pagination::{
        paging_mode::PagingMode,
        policy::{PaginationPolicy, PaginationPolicyError},
    };
    use pretty_assertions::assert_eq;

    fn resolved(config: &RuntimeConfig) -> Result<PaginationPolicy, PaginationPolicyError> {
        resolve_pagination(&config.extensions, config.pagination)
    }

    /// A broker with neither section walks its collections by cursor.
    #[test]
    fn a_default_config_resolves_to_cursor_default() {
        assert_eq!(resolved(&RuntimeConfig::default()), Ok(PaginationPolicy::CursorDefault));
    }

    /// Disabling cursors while keeping them the default is a contradiction the
    /// configuration load refuses.
    #[test]
    fn disabled_cursor_with_cursor_default_fails_validation() {
        let mut config = RuntimeConfig::default();
        config.extensions.cursor_pagination = HttpFeatureMode::Disabled;
        let error = validate_config(&config).expect_err("the pair is refused");
        assert!(
            matches!(
                error,
                ConfigurationError::Pagination(PaginationPolicyError::CursorDefaultWithoutCursorPagination)
            ),
            "{error}"
        );
    }

    /// Disabling cursors with an offset default leaves offsets alone.
    #[test]
    fn disabled_cursor_with_offset_default_resolves_offset_only() {
        let mut config = RuntimeConfig::default();
        config.extensions.cursor_pagination = HttpFeatureMode::Disabled;
        config.pagination.default_mode = PagingMode::Offset;
        assert_eq!(resolved(&config), Ok(PaginationPolicy::OffsetOnly));
        assert!(validate_config(&config).is_ok());
    }

    /// The conformance override forces cursors off without the operator having
    /// contradicted themselves, so the default falls back to offsets.
    #[test]
    fn conformance_mode_with_cursor_default_resolves_offset_only() {
        let mut config = RuntimeConfig::default();
        config.extensions.conformance_mode = HttpFeatureMode::Enabled;
        assert_eq!(resolved(&config), Ok(PaginationPolicy::OffsetOnly));
        assert!(validate_config(&config).is_ok());
    }

    /// An offset default with cursors on keeps the cursor walk available.
    #[test]
    fn an_offset_default_with_cursors_on_resolves_offset_default() {
        let mut config = RuntimeConfig::default();
        config.pagination.default_mode = PagingMode::Offset;
        assert_eq!(resolved(&config), Ok(PaginationPolicy::OffsetDefault));
    }
}
