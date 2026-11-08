//! The query of `IOP_003_01_01` and `IOP_003_01_02`: b1 holds an inclusive registration of the spot counts at b2 and an
//! exclusive registration of the `location` of parking 2 at b3. The cases differ in the binding of the query.

use crate::{
    composition::{combining, expect_among, expect_ids, taking},
    node::Node,
    parking::{AVAILABLE_SPOTS, Kind, LOCATION, SPOT_COUNTS, Sample},
    scenario::{ContextChoice, Scenario},
    topology::INCLUSIVE_AND_EXCLUSIVE,
};
use googletest::prelude::*;
use ngsi_ld::http::operation::QueryMethod;

/// Queries the parkings through b1, b2 and b3 with `method`. Parking 1 lives in b2 and parking 2 in b2 and b3, so b1
/// answers the spot counts of b2 for parking 1, and the `availableSpotsNumber` of b2 and the `location` of b3 for
/// parking 2.
pub(crate) async fn run(context: ContextChoice, method: QueryMethod) -> Result<()> {
    let mut scenario = Scenario::start(context).await?;
    let first_at_b2 = scenario.seed(Node::B2, Sample::Parking1Full).await?;
    let second_at_b2 = scenario.seed(Node::B2, Sample::Parking2WithoutLocationAndTotal).await?;
    let second_at_b3 = scenario.seed(Node::B3, Sample::Parking2Full).await?;
    scenario.register_all(&INCLUSIVE_AND_EXCLUSIVE).await?;
    let both = [first_at_b2.id.clone(), second_at_b2.id.clone()];
    let at_b1 = scenario.query(Node::B1, &[Kind::Parking], &[], method).await?;
    let at_b2 = scenario.query(Node::B2, &[Kind::Parking], &[], method).await?;
    let at_b3 = scenario.query(Node::B3, &[Kind::Parking], &[], method).await?;
    expect_ids(&at_b1, &both)?;
    expect_ids(&at_b2, &both)?;
    expect_ids(&at_b3, &both[1..])?;
    let first_b2 = expect_among(&at_b2, &first_at_b2.id, &first_at_b2)?;
    let second_b2 = expect_among(&at_b2, &second_at_b2.id, &second_at_b2)?;
    let second_b3 = expect_among(&at_b3, &second_at_b3.id, &second_at_b3)?;
    let first_expected = combining(&first_at_b2, &[taking(first_b2, SPOT_COUNTS)])?;
    let second_expected = combining(&second_at_b2, &[taking(second_b2, AVAILABLE_SPOTS), taking(second_b3, LOCATION)])?;
    expect_among(&at_b1, &first_at_b2.id, &first_expected)?;
    expect_among(&at_b1, &second_at_b2.id, &second_expected)?;
    Ok(())
}
