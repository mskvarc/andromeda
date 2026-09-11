//! The outcome of the last distributed operation a registration triggered (ETSI GS CIM 009 Table 5.2.9-2).

use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, IntoStaticStr};

/// Health status of a context source registration (ETSI GS CIM 009 clause 5.2.9, status member).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, EnumString, EnumIter, IntoStaticStr)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum CsrStatus {
    /// The context source is reachable and healthy.
    Ok,
    /// The context source is unreachable or unhealthy.
    Failed,
}
