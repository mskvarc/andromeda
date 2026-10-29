//! ETSI `IOP_003_03_01`: b1 holds an auxiliary registration of every parking at b2 and inclusive registrations of every
//! parking at b3 and of the spot counts at b4. b1 answers parking 1 with the `name` and `location` of b3 and the spot
//! counts of b4, and parking 2, which only b2 holds, as b2 holds it.

use crate::{
    composition::{combining, expect_among, expect_ids, taking},
    node::Node,
    parking::{Kind, LOCATION, NAME, SPOT_COUNTS, Sample},
    payload::Payload,
    scenario::{ContextChoice, Scenario},
};
use googletest::prelude::*;
use ngsi_ld::http::operation::QueryMethod;

async fn run(context: ContextChoice) -> Result<()> {
    let mut scenario = Scenario::start(context).await?;
    let second_at_b2 = scenario.seed(Node::B2, Sample::Parking2WithoutLocation).await?;
    let first_at_b3 = scenario.seed(Node::B3, Sample::Parking1Full).await?;
    let first_at_b4 = scenario.seed(Node::B4, Sample::Parking1WithoutLocation).await?;
    scenario.register(Node::B1, Payload::Auxiliary1, Node::B2).await?;
    scenario.register(Node::B1, Payload::Inclusive1, Node::B3).await?;
    scenario.register(Node::B1, Payload::Inclusive2, Node::B4).await?;
    let both = [first_at_b3.id.clone(), second_at_b2.id.clone()]; // clone: the entities stay whole for the expectations
    let at_b1 = scenario.query(Node::B1, &[Kind::Parking], &[], QueryMethod::Get).await?;
    let at_b2 = scenario.query(Node::B2, &[Kind::Parking], &[], QueryMethod::Get).await?;
    let at_b3 = scenario.query(Node::B3, &[Kind::Parking], &[], QueryMethod::Get).await?;
    let at_b4 = scenario.query(Node::B4, &[Kind::Parking], &[], QueryMethod::Get).await?;
    expect_ids(&at_b1, &both)?;
    expect_ids(&at_b2, &both[1..])?;
    expect_ids(&at_b3, &both[..1])?;
    expect_ids(&at_b4, &both[..1])?;
    let second_b2 = expect_among(&at_b2, &second_at_b2.id, &second_at_b2)?;
    let first_b3 = expect_among(&at_b3, &first_at_b3.id, &first_at_b3)?;
    let first_b4 = expect_among(&at_b4, &first_at_b4.id, &first_at_b4)?;
    let first_expected = combining(
        &first_at_b3,
        &[taking(first_b3, NAME), taking(first_b3, LOCATION), taking(first_b4, SPOT_COUNTS)],
    )?;
    expect_among(&at_b1, &first_at_b3.id, &first_expected)?;
    expect_among(&at_b1, &second_at_b2.id, second_b2)?;
    Ok(())
}

both_contexts!(
    run,
    iop_003_03_01_01_query_parkings_with_default_context,
    iop_003_03_01_02_query_parkings_with_user_context,
    pins = "Check that querying OffStreetParking from b1 answers the name and location of b3 and the spot counts of b4 for parking 1 and b2's parking 2",
    clause = "4.3.3, 9, 10.4.3",
);
