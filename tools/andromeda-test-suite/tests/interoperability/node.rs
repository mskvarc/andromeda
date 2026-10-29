//! The five brokers of the interoperability stack, by the names the ETSI cases give them.

use andromeda_test_suite::config::{BrokerRole, PeerRole};
use strum::{Display, EnumIter};

/// A broker of the stack: b1 is the front broker every case talks to, b2 to b5 the context sources behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Display, EnumIter)]
#[strum(serialize_all = "lowercase")]
pub(crate) enum Node {
    /// The front broker.
    B1,
    /// The second broker.
    B2,
    /// The third broker.
    B3,
    /// The fourth broker.
    B4,
    /// The fifth broker.
    B5,
}

impl Node {
    /// The role the stack's configuration gives the broker as the test process reaches it.
    pub(crate) const fn broker(self) -> BrokerRole {
        match self {
            Node::B1 => BrokerRole::Front,
            Node::B2 => BrokerRole::B,
            Node::B3 => BrokerRole::C,
            Node::B4 => BrokerRole::D,
            Node::B5 => BrokerRole::E,
        }
    }

    /// The role the stack's configuration gives the broker as another broker reaches it.
    pub(crate) const fn peer(self) -> PeerRole {
        match self {
            Node::B1 => PeerRole::A,
            Node::B2 => PeerRole::B,
            Node::B3 => PeerRole::C,
            Node::B4 => PeerRole::D,
            Node::B5 => PeerRole::E,
        }
    }
}
