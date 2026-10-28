//! The three Vehicles of Annex C.5.3 and C.5.4, each holding only its `brandName`: a Mercedes, a Volvo and a Renault.

use andromeda_test_suite::{broker::BrokerClient, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::Entity;
use serde_json::{Value, json};

/// The id suffix and brand of each Vehicle.
pub(crate) const BRANDS: [(&str, &str); 3] = [("A456", "Mercedes"), ("B9211", "Volvo"), ("C7788", "Renault")];

/// Creates the three Vehicles in one batch through `client`, requiring 201.
pub(crate) async fn create_brand_fleet(client: &BrokerClient, namespace: &Namespace) -> Result<()> {
    let fleet = BRANDS
        .iter()
        .map(|(suffix, brand)| {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", suffix)?)
                .entity_type(namespace.entity_type("Vehicle")?)
                .property("brandName", *brand)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    let created = client.with_responses().entities().batch_create(&fleet).send().await;
    verify_that!(&created, answered(StatusCode::CREATED))?;
    Ok(())
}

/// The Vehicle `suffix` of `brand` in the simplified representation, without an `@context`.
pub(crate) fn simplified_brand_vehicle(namespace: &Namespace, suffix: &str, brand: &str) -> Value {
    json!({"id": namespace.urn_text("Vehicle", suffix), "type": namespace.suffixed("Vehicle"), "brandName": brand})
}
