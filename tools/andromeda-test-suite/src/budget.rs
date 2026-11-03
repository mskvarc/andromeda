//! The limits a stack configures its brokers with, by name, as the suite configuration reads them.

use serde::Deserialize;
use strum::IntoStaticStr;

/// A limit the stack configures its brokers with, which a test reaches or crosses on purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum Budget {
    /// The bytes of attribute values one request may carry.
    RequestValue,
    /// The bytes of attribute values one entity may hold.
    EntityValue,
    /// The positions one geometry may hold on the front broker.
    Position,
    /// The positions one geometry may hold on the not-recorded broker, the broker default.
    LargePosition,
    /// The bytes of the largest request body a broker reads.
    MaxRequestBody,
    /// The bytes one page of a collection may render to on the front broker.
    PageBytes,
}
