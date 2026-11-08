//! The query of `IOP_003_04_01` and `IOP_003_04_02`: five brokers; b1 holds an auxiliary registration of the spot
//! counts at b2 and an inclusive registration of every parking at b3; b2 holds redirect registrations of every parking
//! at b4 and of the `location` at b5; b3 holds exclusive registrations of the `location` of parking 2 and of parking 1
//! at b5. The cases differ in the binding of the query.

use crate::{
    composition::{combining, expect_among, expect_ids, taking},
    node::Node,
    parking::{Kind, LOCATION, SPOT_COUNTS, Sample},
    payload::Payload,
    scenario::{ContextChoice, Scenario},
    topology::Link,
};
use googletest::prelude::*;
use ngsi_ld::http::operation::QueryMethod;

/// The registrations of the five brokers.
const TOPOLOGY: [Link; 6] = [
    (Node::B1, Payload::Auxiliary2, Node::B2),
    (Node::B2, Payload::Redirect1, Node::B4),
    (Node::B2, Payload::Redirect2, Node::B5),
    (Node::B1, Payload::Inclusive1, Node::B3),
    (Node::B3, Payload::Exclusive2, Node::B5),
    (Node::B3, Payload::Exclusive3, Node::B5),
];

/// Queries the parkings through b1, b4 and b5 with `method`. b1 answers the `location` of b5 for parking 1, and the
/// spot counts of b4 and the `location` of b5 for parking 2.
pub(crate) async fn run(context: ContextChoice, method: QueryMethod) -> Result<()> {
    let mut scenario = Scenario::start(context).await?;
    let first_at_b4 = scenario.seed(Node::B4, Sample::Parking1LocationAndName).await?;
    let second_at_b4 = scenario.seed(Node::B4, Sample::Parking2Full).await?;
    let first_at_b5 = scenario.seed(Node::B5, Sample::Parking1Full).await?;
    let second_at_b5 = scenario.seed(Node::B5, Sample::Parking2LocationAndName).await?;
    scenario.register_all(&TOPOLOGY).await?;
    let both = [first_at_b4.id.clone(), second_at_b4.id.clone()];
    let at_b1 = scenario.query(Node::B1, &[Kind::Parking], &[], method).await?;
    let at_b4 = scenario.query(Node::B4, &[Kind::Parking], &[], method).await?;
    let at_b5 = scenario.query(Node::B5, &[Kind::Parking], &[], method).await?;
    expect_ids(&at_b1, &both)?;
    expect_ids(&at_b4, &both)?;
    expect_ids(&at_b5, &both)?;
    expect_among(&at_b4, &first_at_b4.id, &first_at_b4)?;
    let second_b4 = expect_among(&at_b4, &second_at_b4.id, &second_at_b4)?;
    let first_b5 = expect_among(&at_b5, &first_at_b5.id, &first_at_b5)?;
    let second_b5 = expect_among(&at_b5, &second_at_b5.id, &second_at_b5)?;
    expect_among(&at_b1, &first_at_b5.id, &combining(&first_at_b5, &[taking(first_b5, LOCATION)])?)?;
    expect_among(
        &at_b1,
        &second_at_b4.id,
        &combining(&second_at_b4, &[taking(second_b4, SPOT_COUNTS), taking(second_b5, LOCATION)])?,
    )?;
    Ok(())
}
