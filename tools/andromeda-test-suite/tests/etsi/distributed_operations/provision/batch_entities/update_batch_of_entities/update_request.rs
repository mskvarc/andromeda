//! The Batch Entity Update the update cases send, and the fragments it carries for the Vehicles `-1` and `-2`.

use crate::entity_operations::ld_json_body_client;
use andromeda_test_suite::{broker::BrokerClient, namespace::Namespace, problem::answered, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::Overwrite,
    model::{Complete, DatasetId, Entity, EntityFragment, Property, Relationship, Value},
};

/// The fragment of the Vehicle `urn:ngsi-ld:Vehicle:<ns>-<n>`: the scope `/Madrid/Centro`, an `isParked` Relationship
/// to the `OffStreetParking` Downtown2 observed at 2017-07-29T15:00:04Z and provided by Bob, and two `speed`
/// instances, 55 from a Speedometer as the default instance and 54.5 from a GPS in the dataset
/// `urn:ngsi-ld:Property:<ns>-gpsBxyz123-speed`.
fn parked_speed_fragment(namespace: &Namespace, n: u32) -> Result<EntityFragment<Complete>> {
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", n)?)
        .entity_type("Vehicle")
        .scope("/Madrid/Centro")
        .relationship(
            "isParked",
            Relationship::builder(namespace.urn("OffStreetParking", "Downtown2")?)
                .observed_at("2017-07-29T15:00:04Z".parse()?)
                .relationship("providedBy", namespace.urn("Person", "Bob")?),
        )
        .property("speed", Property::builder(55).property("source", "Speedometer"))
        .property(
            "speed",
            Property::builder(Value::try_from(54.5)?)
                .property("source", "GPS")
                .dataset_id(namespace.urn_text("Property", "gpsBxyz123-speed").parse::<DatasetId>()?),
        )
        .build()?
        .into_fragment())
}

/// `Batch Update Entities` of the fragments of the Vehicles `-1` and `-2` with `overwrite`, as `application/ld+json`
/// with the vocabulary as a one-element `@context` list in each fragment: requires 204.
pub(crate) async fn update_vehicles(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace, overwrite: Overwrite) -> Result<()> {
    let fragments = [parked_speed_fragment(namespace, 1)?, parked_speed_fragment(namespace, 2)?];
    let answer = ld_json_body_client(broker, vocabulary)?
        .with_responses()
        .entities()
        .batch_update(&fragments)
        .overwrite(overwrite)
        .send()
        .await;
    verify_that!(&answer, answered(StatusCode::NO_CONTENT))
}
