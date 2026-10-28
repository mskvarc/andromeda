//! The temporal evolution of the Vehicle B9211 of Annex C.5.5, C.5.6 and C.5.14: its `brandName` observed once at
//! 11:00 and three `speed` readings between 12:03 and 12:07.

use andromeda_test_suite::{broker::BrokerClient, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::model::{DateTime, EntityId, Property, evolution::TemporalEntity};

/// The speed readings of B9211 and the instants they are observed at.
pub(crate) const SPEEDS: [(i64, &str); 3] = [(120, "2018-08-01T12:03:00Z"), (80, "2018-08-01T12:05:00Z"), (100, "2018-08-01T12:07:00Z")];

/// Records the evolution of B9211 through `client` and answers its id.
pub(crate) async fn record_b9211(client: &BrokerClient, namespace: &Namespace) -> Result<EntityId> {
    let mut evolution = TemporalEntity::builder()
        .id(namespace.urn("Vehicle", "B9211")?)
        .entity_type(namespace.entity_type("Vehicle")?)
        .property("brandName", Property::builder("Volvo").observed_at("2018-08-01T11:00:00Z".parse::<DateTime>()?));
    for (speed, at) in SPEEDS {
        evolution = evolution.property("speed", Property::builder(speed).observed_at(at.parse::<DateTime>()?));
    }
    let evolution = evolution.build()?;
    client.temporal().create_or_update(&evolution).send().await?;
    Ok(evolution.id)
}
