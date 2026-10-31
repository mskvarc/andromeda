//! The four data domains one hub federates in the distributed load tests: brokers B to E of the multi stack, each
//! holding its own share of the data and registered at the front broker, the hub, as a context source of it.
//!
//! Broker F gathers at most a handful of entities for one forwarded query, so it is no domain: a domain answers the
//! hub with its whole share.

use crate::config::{BrokerRole, PeerRole};

/// One data domain of the hub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Domain {
    /// The broker holding the domain's data, as the test process reaches it.
    pub broker: BrokerRole,
    /// The same broker as the hub reaches it: the `endpoint` of the domain's registration.
    pub peer: PeerRole,
    /// The domain's name, a single scope level: the suffix of its registration id, the infix of the ids of the
    /// entities it holds and the last level of its scope.
    pub name: &'static str,
}

/// The four domains, in the order a test registers them.
pub const DOMAINS: [Domain; 4] = [
    Domain {
        broker: BrokerRole::B,
        peer: PeerRole::B,
        name: "Madrid",
    },
    Domain {
        broker: BrokerRole::C,
        peer: PeerRole::C,
        name: "Berlin",
    },
    Domain {
        broker: BrokerRole::D,
        peer: PeerRole::D,
        name: "Nordic",
    },
    Domain {
        broker: BrokerRole::E,
        peer: PeerRole::E,
        name: "North",
    },
];

#[cfg(test)]
mod tests {
    use crate::hub_domains::DOMAINS;
    use googletest::prelude::*;
    use std::collections::BTreeSet;

    #[gtest]
    fn the_four_domains_are_four_distinct_brokers_with_distinct_names() {
        expect_that!(DOMAINS.iter().map(|domain| domain.broker).collect::<BTreeSet<_>>(), len(eq(4)));
        expect_that!(DOMAINS.iter().map(|domain| domain.name).collect::<BTreeSet<_>>(), len(eq(4)));
    }
}
