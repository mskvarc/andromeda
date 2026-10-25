//! Construction of the security-header (helmet) middleware layer.

use crate::config::{feature_mode::HttpFeatureMode, helmet::HelmetConfig};
use axum_helmet::{Helmet, HelmetError, HelmetLayer};

/// Build the security-header layer, or `None` when the policy disables it.
///
/// The layer is fallible because `axum-helmet` renders every header value up
/// front and reports an invalid one as [`HelmetError`]; building it once at
/// startup surfaces that failure before the broker serves traffic.
///
/// # Errors
///
/// Returns [`HelmetError`] when the default header set cannot be rendered into a
/// layer.
pub(crate) fn build_helmet_layer(config: HelmetConfig) -> Result<Option<HelmetLayer>, HelmetError> {
    match config.mode {
        HttpFeatureMode::Disabled => Ok(None),
        HttpFeatureMode::Enabled => Helmet::default().try_into().map(Some),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        config::{feature_mode::HttpFeatureMode, helmet::HelmetConfig},
        stack::helmet_layer::build_helmet_layer,
    };

    #[test]
    fn an_enabled_policy_builds_a_layer() {
        let layer = build_helmet_layer(HelmetConfig {
            mode: HttpFeatureMode::Enabled,
        })
        .expect("default helmet builds");
        assert!(layer.is_some());
    }

    #[test]
    fn a_disabled_policy_builds_no_layer() {
        let layer = build_helmet_layer(HelmetConfig {
            mode: HttpFeatureMode::Disabled,
        })
        .expect("disabled helmet builds");
        assert!(layer.is_none());
    }
}
