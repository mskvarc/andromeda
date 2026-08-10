//! Federation operation configuration.

use confique::Config;
use serde::Serialize;
use thiserror::Error;

/// Invalid federation configuration.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum FederationConfigError {
    /// Forwarding requires at least one execution slot.
    #[error("federation.forward_concurrency must be at least 1")]
    ZeroForwardConcurrency,
}

/// Distributed Context Source forwarding limits.
#[derive(Config, Debug, Clone, Serialize)]
pub struct FederationConfig {
    /// Maximum parallel Context Source forwarding requests.
    #[config(env = "ANDROMEDA_FEDERATION_FORWARD_CONCURRENCY", default = 16)]
    pub forward_concurrency: usize,
}

impl FederationConfig {
    /// Validate Context Source forwarding limits.
    pub const fn validate(&self) -> Result<(), FederationConfigError> {
        if self.forward_concurrency == 0 {
            Err(FederationConfigError::ZeroForwardConcurrency)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FederationConfig, FederationConfigError};

    #[test]
    fn zero_forward_concurrency_is_rejected() {
        assert_eq!(
            FederationConfig { forward_concurrency: 0 }.validate(),
            Err(FederationConfigError::ZeroForwardConcurrency)
        );
    }
}
