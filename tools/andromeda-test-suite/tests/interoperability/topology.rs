//! The registrations several ETSI cases share, as lists of `(hub, file, spoke)`: the registration of the file is made
//! on the hub broker and points at the spoke broker.

use crate::{node::Node, payload::Payload};

/// A registration of a data file on `hub` pointing at `spoke`.
pub(crate) type Link = (Node, Payload, Node);

/// b1 holds an inclusive registration of the spot counts at b2 and an exclusive registration of the `location` of
/// parking 2 at b3.
pub(crate) const INCLUSIVE_AND_EXCLUSIVE: [Link; 2] = [(Node::B1, Payload::Inclusive2, Node::B2), (Node::B1, Payload::Exclusive2, Node::B3)];

/// b1 holds an inclusive registration of the spot counts at b2 and redirect registrations of the `location` at b3 and
/// b4.
pub(crate) const INCLUSIVE_AND_TWO_REDIRECTS: [Link; 3] = [
    (Node::B1, Payload::Inclusive2, Node::B2),
    (Node::B1, Payload::Redirect2, Node::B3),
    (Node::B1, Payload::Redirect2, Node::B4),
];

/// b1 holds an auxiliary registration of the spot counts at b2, an inclusive registration of every parking at b3 and
/// an inclusive registration of the spot counts at b4.
pub(crate) const AUXILIARY_AND_TWO_INCLUSIVES: [Link; 3] = [
    (Node::B1, Payload::Auxiliary2, Node::B2),
    (Node::B1, Payload::Inclusive1, Node::B3),
    (Node::B1, Payload::Inclusive2, Node::B4),
];

/// b1 holds an auxiliary registration of the spot counts at b2 and an inclusive registration of every parking at b3;
/// b2 holds redirect registrations of every parking at b4 and of the `location` at b5; b3 holds an exclusive
/// registration of the `totalSpotsNumber` of parking 2 at b5.
pub(crate) const FIVE_BROKERS_WITH_TOTAL_SPOTS_AT_B5: [Link; 5] = [
    (Node::B1, Payload::Auxiliary2, Node::B2),
    (Node::B2, Payload::Redirect1, Node::B4),
    (Node::B2, Payload::Redirect2, Node::B5),
    (Node::B1, Payload::Inclusive1, Node::B3),
    (Node::B3, Payload::Exclusive1, Node::B5),
];
