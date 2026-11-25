//! The brokers of a stack as other brokers of the stack reach them, by the role the stack gives each: the address a
//! registration `endpoint` names, read from `ANDROMEDA_TEST_SUITE__PEERS__<ROLE>`.

use serde::Deserialize;
use strum::IntoStaticStr;

/// A broker as another broker of the stack reaches it: what a registration `endpoint` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum PeerRole {
    /// Broker A of the multi-broker stacks.
    A,
    /// Broker B of the multi-broker stacks.
    B,
    /// Broker C of the multi-broker stacks.
    C,
    /// Broker D of the multi-broker stacks.
    D,
    /// Broker E of the multi-broker stacks.
    E,
    /// Broker F of the multi stack.
    F,
}
