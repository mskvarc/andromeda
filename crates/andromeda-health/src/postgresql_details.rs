//! What the PostgreSQL pool reports about itself.

use serde::{Deserialize, Serialize};

/// The outcome of pinging the database through its pool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub enum PostgreSqlDetails {
    /// The ping was answered.
    Reachable {
        /// Idle connections in the pool.
        idle_connections: usize,
        /// Round-trip time of the ping, in milliseconds.
        ping_latency_ms: u64,
        /// Connections currently open in the pool.
        pool_size: u32,
    },
    /// The ping failed.
    Unreachable {
        /// Why the ping failed.
        error: String,
    },
}

#[cfg(test)]
mod tests {
    use crate::postgresql_details::PostgreSqlDetails;
    use serde_json::json;

    #[test]
    fn a_reachable_pool_serialises_its_statistics_as_numbers() {
        let details = PostgreSqlDetails::Reachable {
            idle_connections: 3,
            ping_latency_ms: 2,
            pool_size: 5,
        };
        pretty_assertions::assert_eq!(
            serde_json::to_value(&details).expect("serialises"),
            json!({"idleConnections": 3, "pingLatencyMs": 2, "poolSize": 5})
        );
    }

    #[test]
    fn an_unreachable_pool_serialises_only_its_error() {
        let details = PostgreSqlDetails::Unreachable {
            error: "connection refused".to_owned(),
        };
        pretty_assertions::assert_eq!(serde_json::to_value(&details).expect("serialises"), json!({"error": "connection refused"}));
    }

    #[test]
    fn each_variant_reads_back_from_its_own_members() {
        let unreachable: PostgreSqlDetails = serde_json::from_value(json!({"error": "down"})).expect("parses");
        pretty_assertions::assert_eq!(unreachable, PostgreSqlDetails::Unreachable { error: "down".to_owned() });
    }
}
