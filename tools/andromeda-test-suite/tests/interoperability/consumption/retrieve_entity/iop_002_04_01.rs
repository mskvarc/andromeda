//! ETSI `IOP_002_04_01`: five brokers; b1 answers parking 1 with the spot counts b4 holds, reached through b2, and the
//! `location` b5 holds, reached through b3.
//!
//! The published case loads, on b3, the exclusive registration of the `totalSpotsNumber` of parking 2. With it no
//! registration carries b5's `location` of parking 1 to b1: b3 holds no parking 1 and its one registration names
//! another entity, and b2's redirect of the `location` is out of reach, since b1 asks b2 only for the spot counts its
//! auxiliary registration names (clause 4.3.6.1). The case's setup asks for one exclusive registration from b3 to b5,
//! which `IOP_002_04_02` fills with the exclusive registration of the `location` of parking 1; this port loads that
//! one, so b1 receives the `location` through b3.

use crate::{
    composition::{combining, expect_same, taking},
    node::Node,
    parking::{Kind, LOCATION, SPOT_COUNTS, Sample},
    payload::Payload,
    scenario::{ContextChoice, Scenario},
    topology::Link,
};
use googletest::prelude::*;

/// b1 holds an auxiliary registration of the spot counts at b2 and an inclusive registration of every parking at b3;
/// b2 holds redirect registrations of every parking at b4 and of the `location` at b5; b3 holds an exclusive
/// registration of the `location` of parking 1 at b5.
const TOPOLOGY: [Link; 5] = [
    (Node::B1, Payload::Auxiliary2, Node::B2),
    (Node::B2, Payload::Redirect1, Node::B4),
    (Node::B2, Payload::Redirect2, Node::B5),
    (Node::B1, Payload::Inclusive1, Node::B3),
    (Node::B3, Payload::Exclusive3, Node::B5),
];

async fn run(context: ContextChoice) -> Result<()> {
    let mut scenario = Scenario::start(context).await?;
    let counted = scenario.seed(Node::B4, Sample::Parking1WithoutLocation).await?;
    let full = scenario.seed(Node::B5, Sample::Parking1Full).await?;
    scenario.register_all(&TOPOLOGY).await?;
    let at_b1 = scenario.retrieve(Node::B1, &counted.id, Kind::Parking).await?;
    let at_b4 = scenario.retrieve(Node::B4, &counted.id, Kind::Parking).await?;
    let at_b5 = scenario.retrieve(Node::B5, &counted.id, Kind::Parking).await?;
    expect_same(&at_b4, &counted)?;
    expect_same(&at_b5, &full)?;
    expect_same(&at_b1, &combining(&counted, &[taking(&at_b4, SPOT_COUNTS), taking(&at_b5, LOCATION)])?)
}

both_contexts!(
    run,
    iop_002_04_01_01_retrieve_parking_1_with_default_context,
    iop_002_04_01_02_retrieve_parking_1_with_user_context,
    pins = "Check that retrieving OffStreetParking:1 from b1 over five brokers answers the spot counts of b4 and the location of b5",
    clause = "4.3.3, 9, 10.4.2",
);
