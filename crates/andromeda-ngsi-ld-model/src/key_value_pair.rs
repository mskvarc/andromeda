//! The KeyValuePair member (ETSI GS CIM 009 clause 5.2.22).
//!
//! Two resources carry a list of these and mean the same thing by it: a
//! subscription's `receiverInfo` and `notifierInfo` (clause 5.2.15), and a
//! snapshot's `receiverInfo` (clause 5.2.41). Both are consumed by whatever
//! delivers the notification -- as HTTP headers, as MQTT user properties, or
//! as the MQTT envelope's metadata object.

use compact_str::CompactString;
use serde::{Deserialize, Serialize};

/// One `{"key": ..., "value": ...}` entry.
///
/// Clause 5.2.22 defines both members as Strings, which is what makes a
/// malformed entry a request the broker must reject rather than one it may
/// partially read.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyValuePair {
    /// The entry's key.
    pub key: CompactString,
    /// The entry's value.
    pub value: CompactString,
}

#[cfg(test)]
mod tests {
    use crate::key_value_pair::KeyValuePair;
    use serde_json::json;

    #[test]
    fn a_pair_round_trips_through_the_wire_shape_clause_5_2_22_defines() {
        let pair: KeyValuePair = serde_json::from_value(json!({"key": "Authorization", "value": "Bearer token"})).expect("parses");
        pretty_assertions::assert_eq!(pair.key, "Authorization");
        pretty_assertions::assert_eq!(pair.value, "Bearer token");
        pretty_assertions::assert_eq!(
            serde_json::to_value(&pair).expect("serialises"),
            json!({"key": "Authorization", "value": "Bearer token"})
        );
    }

    #[test]
    fn an_entry_missing_its_value_is_rejected_rather_than_read_as_a_key_alone() {
        assert!(serde_json::from_value::<KeyValuePair>(json!({"key": "MQTT-QoS"})).is_err());
    }

    #[test]
    fn a_non_string_value_is_rejected_because_clause_5_2_22_defines_both_members_as_strings() {
        assert!(serde_json::from_value::<KeyValuePair>(json!({"key": "MQTT-QoS", "value": 2})).is_err());
    }
}
