//! The gauge naming the version of the running broker.

use crate::names::{BUILD_INFO, LABEL_VERSION};
use metrics::gauge;

/// The broker release the build-info gauge is labelled with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrokerVersion(&'static str);

impl BrokerVersion {
    /// The version `version`, as the build names it (`CARGO_PKG_VERSION`).
    #[must_use]
    pub const fn from_static(version: &'static str) -> BrokerVersion {
        BrokerVersion(version)
    }

    /// Set the build-info gauge to 1 under this version.
    pub(crate) fn record(self) {
        gauge!(BUILD_INFO, LABEL_VERSION => self.0).set(1.0);
    }
}
