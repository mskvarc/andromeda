//! The mode a Context Source Registration forwards under (ETSI GS CIM 009 clauses 4.3.6 and 5.2.9).

use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, IntoStaticStr};

/// Forwarding mode for distributed operations (ETSI GS CIM 009 clause 4.3.6 / 5.2.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, Display, EnumString, EnumIter, IntoStaticStr)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum ForwardingMode {
    /// The registration supplements local data (default).
    #[default]
    Inclusive,
    /// The registration is the exclusive source for matched entities.
    Exclusive,
    /// Requests are redirected to the context source.
    Redirect,
    /// The registration provides auxiliary/fallback data.
    Auxiliary,
}
