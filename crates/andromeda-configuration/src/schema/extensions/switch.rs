//! The table of an extension that is only switched on or off.

use crate::schema::feature_mode::FeatureMode;
use serde::{Deserialize, Serialize};
use toml_comment::TomlComment;

/// Whether one extension is served. Off by default: a broker that sets no
/// extension is a conformant broker.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExtensionSwitch {
    /// Whether the extension is served: `enabled` or `disabled`.
    #[toml_comment(inline)]
    pub mode: FeatureMode,
}

impl ExtensionSwitch {
    /// A switch set to `mode`.
    #[must_use]
    pub const fn new(mode: FeatureMode) -> ExtensionSwitch {
        ExtensionSwitch { mode }
    }

    /// Whether the extension was switched on, before the conformance override.
    #[must_use]
    pub const fn is_requested(self) -> bool {
        matches!(self.mode, FeatureMode::Enabled)
    }
}

impl Default for ExtensionSwitch {
    // Manual: the switch is off by default, while a bare `FeatureMode`
    // defaults to on.
    fn default() -> ExtensionSwitch {
        ExtensionSwitch::new(FeatureMode::Disabled)
    }
}
