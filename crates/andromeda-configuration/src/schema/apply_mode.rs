//! Whether a running broker takes up a changed setting without a restart.

use serde::{Deserialize, Serialize};

/// How a change to a key reaches a running broker.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApplyMode {
    /// The broker reads the key at start; a change takes a restart.
    #[default]
    Restart,
    /// The broker takes up a changed value while it runs.
    Reload,
}
