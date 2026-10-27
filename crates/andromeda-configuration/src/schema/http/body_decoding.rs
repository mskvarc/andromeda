//! Where request payloads are decoded: on an executor worker or on the
//! blocking thread pool.

use crate::human_byte_size::HumanByteSize;
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;
use thiserror::Error;
use toml_comment::TomlComment;

/// The default most payloads decoded beside the executor at once.
const DEFAULT_MAX_CONCURRENT_BLOCKING: NonZeroUsize = NonZeroUsize::MIN.saturating_add(3);

/// An unusable `[http.body_decoding]` table.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum BodyDecodingConfigError {
    /// A threshold beyond the platform address width would never send a
    /// payload to the blocking pool.
    #[error("http.body_decoding.blocking_threshold does not fit the platform address width")]
    BlockingThresholdExceedsAddressWidth,
}

/// Where request payloads are decoded.
#[derive(Deserialize, Serialize, TomlComment, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BodyDecodingConfig {
    /// The payload size, in human-readable form (`"256 KiB"`), from which a
    /// payload is parsed and decoded on the blocking thread pool instead of
    /// on an executor worker, so a large payload does not stall unrelated
    /// requests sharing that worker.
    #[toml_comment(inline)]
    pub blocking_threshold: HumanByteSize,
    /// The most payloads decoded on the blocking thread pool at once. A
    /// decode holds memory in proportion to its payload while it runs; a
    /// payload waiting for its turn holds only its bytes.
    #[toml_comment(inline)]
    pub max_concurrent_blocking: NonZeroUsize,
}

impl Default for BodyDecodingConfig {
    fn default() -> BodyDecodingConfig {
        BodyDecodingConfig {
            blocking_threshold: HumanByteSize::kib(256),
            max_concurrent_blocking: DEFAULT_MAX_CONCURRENT_BLOCKING,
        }
    }
}

impl BodyDecodingConfig {
    /// The blocking threshold in bytes.
    ///
    /// # Errors
    ///
    /// Returns [`BodyDecodingConfigError::BlockingThresholdExceedsAddressWidth`]
    /// when the threshold does not fit a `usize`.
    pub fn blocking_threshold_bytes(&self) -> Result<usize, BodyDecodingConfigError> {
        usize::try_from(self.blocking_threshold.as_u64()).map_err(|_| BodyDecodingConfigError::BlockingThresholdExceedsAddressWidth)
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::http::body_decoding::BodyDecodingConfig;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_default_decodes_from_256_kib_on_four_blocking_threads() {
        let config = BodyDecodingConfig::default();
        assert_eq!(config.blocking_threshold_bytes(), Ok(262_144));
        assert_eq!(config.max_concurrent_blocking.get(), 4);
    }
}
