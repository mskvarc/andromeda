//! ETSI `IOP_003_02_01`: b1 holds an inclusive registration of the spot counts at b2, a redirect registration of the
//! `location` at b3 and a redirect registration of the `location` of parking 2 at b4. b1 answers the spot counts of b2
//! and the `location` of b3 for parking 1, and the spot counts of b2 and the `location` of b4 for parking 2.
//!
//! The case documentation names b3 as the source of the spot counts of parking 2; its checks, and the registrations,
//! take them from b2, as the redirect registrations cover the `location` alone.

use crate::{
    composition::{combining, expect_among, expect_ids, taking},
    node::Node,
    parking::{Kind, LOCATION, SPOT_COUNTS, Sample},
    payload::Payload,
    scenario::{ContextChoice, Scenario},
};
use googletest::prelude::*;
use ngsi_ld::http::operation::QueryMethod;

async fn run(context: ContextChoice) -> Result<()> {
    let mut scenario = Scenario::start(context).await?;
    let first_at_b2 = scenario.seed(Node::B2, Sample::Parking1WithoutLocation).await?;
    let second_at_b2 = scenario.seed(Node::B2, Sample::Parking2WithoutLocation).await?;
    let first_at_b3 = scenario.seed(Node::B3, Sample::Parking1Full).await?;
    let second_at_b3 = scenario.seed(Node::B3, Sample::Parking2Full).await?;
    let second_at_b4 = scenario.seed(Node::B4, Sample::Parking2LocationAndName).await?;
    scenario.register(Node::B1, Payload::Inclusive2, Node::B2).await?;
    scenario.register(Node::B1, Payload::Redirect2, Node::B3).await?;
    scenario.register(Node::B1, Payload::Redirect3, Node::B4).await?;
    let both = [first_at_b2.id.clone(), second_at_b2.id.clone()]; // clone: the entities stay whole for the expectations
    let at_b1 = scenario.query(Node::B1, &[Kind::Parking], &[], QueryMethod::Get).await?;
    let at_b2 = scenario.query(Node::B2, &[Kind::Parking], &[], QueryMethod::Get).await?;
    let at_b3 = scenario.query(Node::B3, &[Kind::Parking], &[], QueryMethod::Get).await?;
    let at_b4 = scenario.query(Node::B4, &[Kind::Parking], &[], QueryMethod::Get).await?;
    expect_ids(&at_b1, &both)?;
    expect_ids(&at_b2, &both)?;
    expect_ids(&at_b3, &both)?;
    expect_ids(&at_b4, &both[1..])?;
    let first_b2 = expect_among(&at_b2, &first_at_b2.id, &first_at_b2)?;
    let second_b2 = expect_among(&at_b2, &second_at_b2.id, &second_at_b2)?;
    let first_b3 = expect_among(&at_b3, &first_at_b3.id, &first_at_b3)?;
    expect_among(&at_b3, &second_at_b3.id, &second_at_b3)?;
    let second_b4 = expect_among(&at_b4, &second_at_b4.id, &second_at_b4)?;
    let first_expected = combining(&first_at_b2, &[taking(first_b2, SPOT_COUNTS), taking(first_b3, LOCATION)])?;
    let second_expected = combining(&second_at_b2, &[taking(second_b2, SPOT_COUNTS), taking(second_b4, LOCATION)])?;
    expect_among(&at_b1, &first_at_b2.id, &first_expected)?;
    expect_among(&at_b1, &second_at_b2.id, &second_expected)?;
    Ok(())
}

both_contexts!(
    run,
    iop_003_02_01_01_query_parkings_with_default_context,
    iop_003_02_01_02_query_parkings_with_user_context,
    pins = "Check that querying OffStreetParking from b1 answers the spot counts of b2 and the location of b3 for parking 1 and of b4 for parking 2",
    clause = "4.3.3, 9, 10.4.3",
);
