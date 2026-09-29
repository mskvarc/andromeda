//! Whether Core API writes add to the Temporal Evolution of an Entity.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Whether a write through the Core API is recorded in the Temporal Evolution
/// of the Entity it changes.
///
/// Clause 4.3.1 leaves the choice to the broker: "Brokers that intend to
/// implement the Temporal API should consider updating the Temporal Evolution
/// of an Entity whenever the 'current state' is modified via the Core API."
/// Either way, a Core write still changes current state, and a Temporal API
/// write (clauses 5.6.11 to 5.6.16) is recorded exactly as before.
#[derive(Clone, Copy, Debug, Default, Deserialize, Display, EnumString, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum CoreWriteHistory {
    /// Every Core write that changes current state records the instances it
    /// wrote, the instances it removed and the Scope it cleared.
    #[default]
    Recorded,
    /// Core writes leave the Temporal Evolution untouched: only the Temporal
    /// API adds to it or removes from it.
    NotRecorded,
}

#[cfg(test)]
mod tests {
    use crate::core_write_history::CoreWriteHistory;
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    #[test]
    fn core_writes_are_recorded_unless_an_operator_says_otherwise() {
        assert_eq!(CoreWriteHistory::default(), CoreWriteHistory::Recorded);
    }

    #[test]
    fn the_configuration_values_are_kebab_case() {
        assert_eq!(CoreWriteHistory::from_str("recorded"), Ok(CoreWriteHistory::Recorded));
        assert_eq!(CoreWriteHistory::from_str("not-recorded"), Ok(CoreWriteHistory::NotRecorded));
        assert_eq!(CoreWriteHistory::NotRecorded.to_string(), "not-recorded");
        assert_eq!(
            serde_json::from_value::<CoreWriteHistory>(serde_json::json!("not-recorded")).ok(),
            Some(CoreWriteHistory::NotRecorded)
        );
        assert!(CoreWriteHistory::from_str("off").is_err());
    }
}
