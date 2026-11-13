//! What the database indicator reports about the platform database and the
//! platform pool.

use serde::{Deserialize, Serialize};

/// The outcome of pinging the platform database over the indicator's own
/// connection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub enum PostgreSqlDetails {
    /// The ping was answered. The platform pool is reported beside it: the
    /// ping never borrows from that pool, so a pool with no connection to
    /// spare shows here and leaves the component up.
    Reachable {
        /// Idle connections in the platform pool.
        idle_connections: usize,
        /// The most connections the platform pool opens. The pool is saturated
        /// while `pool_size` is at it and `idle_connections` is zero.
        max_connections: u32,
        /// Round-trip time of the ping, in milliseconds, opening the
        /// indicator's connection included when it had none open.
        ping_latency_ms: u64,
        /// Connections currently open in the platform pool.
        pool_size: u32,
    },
    /// The ping failed or went unanswered.
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
            max_connections: 8,
            ping_latency_ms: 2,
            pool_size: 5,
        };
        pretty_assertions::assert_eq!(
            serde_json::to_value(&details).expect("serialises"),
            json!({"idleConnections": 3, "maxConnections": 8, "pingLatencyMs": 2, "poolSize": 5})
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
        let reachable: PostgreSqlDetails =
            serde_json::from_value(json!({"idleConnections": 0, "maxConnections": 2, "pingLatencyMs": 1, "poolSize": 2})).expect("parses");
        pretty_assertions::assert_eq!(
            reachable,
            PostgreSqlDetails::Reachable {
                idle_connections: 0,
                max_connections: 2,
                ping_latency_ms: 1,
                pool_size: 2,
            }
        );
    }
}
