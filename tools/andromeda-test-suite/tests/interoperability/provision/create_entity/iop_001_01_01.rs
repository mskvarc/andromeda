//! ETSI `IOP_001_01_01`: b1 holds an inclusive registration of the spot counts at b2 and an exclusive registration of
//! the `location` of parking 2 at b3. Parking 1 is created whole in b1, with its spot counts alone in b2 and not in b3.

use crate::{
    node::Node,
    parking::{SPOT_COUNTS, Sample},
    provision::create_entity::{Holds, create_at_b1},
    scenario::{ContextChoice, Scenario},
    topology::INCLUSIVE_AND_EXCLUSIVE,
};
use googletest::prelude::*;

async fn run(context: ContextChoice) -> Result<()> {
    let mut scenario = Scenario::start(context).await?;
    scenario.register_all(&INCLUSIVE_AND_EXCLUSIVE).await?;
    create_at_b1(
        &mut scenario,
        Sample::Parking1Full,
        &[(Node::B1, Holds::Whole), (Node::B2, Holds::Only(SPOT_COUNTS)), (Node::B3, Holds::Nothing)],
    )
    .await
}

both_contexts!(
    run,
    iop_001_01_01_01_create_parking_1_with_default_context,
    iop_001_01_01_02_create_parking_1_with_user_context,
    pins = "Check that creating OffStreetParking:1 at b1 creates it whole in b1, with its spot counts alone in b2 and not in b3",
    clause = "4.3.3, 9, 10.2.2",
);
