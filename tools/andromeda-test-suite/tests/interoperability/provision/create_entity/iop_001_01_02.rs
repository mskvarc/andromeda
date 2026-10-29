//! ETSI `IOP_001_01_02`: b1 holds an inclusive registration of the spot counts at b2 and an exclusive registration of
//! the `location` of parking 2 at b3. Parking 2 is created in b1 and b2 without its `location`, and in b3 with its
//! `location` alone.

use crate::{
    node::Node,
    parking::{LOCATION, SPOT_COUNTS, Sample},
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
        Sample::Parking2Full,
        &[
            (Node::B1, Holds::Without(LOCATION)),
            (Node::B2, Holds::Only(SPOT_COUNTS)),
            (Node::B3, Holds::Only(LOCATION)),
        ],
    )
    .await
}

both_contexts!(
    run,
    iop_001_01_02_01_create_parking_2_with_default_context,
    iop_001_01_02_02_create_parking_2_with_user_context,
    pins = "Check that creating OffStreetParking:2 at b1 creates it without its location in b1 and b2 and with its location alone in b3",
    clause = "4.3.3, 9, 10.2.2",
);
