//! Annex C.5.2: Create Entity of the C.2.2 Vehicle answers 201 with the `Location` of the created Entity (clause
//! 5.6.1).

use crate::{
    served_context::{ServedContexts, linked},
    vehicle_example::{aggregated_context, vehicle},
};
use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, wire::header_value};
use googletest::prelude::*;
use http::{StatusCode, header::LOCATION};

/// Pins: Create Entity of the C.2.2 Vehicle answers 201 with the Location C.5.2.2 prints
/// Clause: C.5.2, 5.6.1, 6.4.3.1
#[gtest]
#[tokio::test]
async fn c_5_2_0001_create_answers_the_printed_location() -> Result<()> {
    let namespace = Namespace::fresh();
    let served = ServedContexts::start(&namespace).await?;
    let broker = linked(&broker::front()?, &aggregated_context(&served, &namespace).await?)?;
    let vehicle = vehicle(&namespace, None)?;
    let created = broker.with_responses().entities().create(&vehicle).send().await;
    verify_that!(&created, answered(StatusCode::CREATED))?;
    expect_that!(
        header_value(created?.head.headers(), &LOCATION),
        some(eq(format!("/ngsi-ld/v1/entities/{}", vehicle.id).as_str()))
    );
    served.stop();
    Ok(())
}
