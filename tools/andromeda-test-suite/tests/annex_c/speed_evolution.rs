//! The temporal evolution of the Vehicle B9211 of Annex C.5.5, C.5.6 and C.5.14: its `brandName` observed once and
//! three `speed` readings between 12:03 and 12:07.
//!
//! The annex observes `brandName` at 11:00 and still selects B9211 by `brandName!="Mercedes"` over the window from
//! 12:00 to 13:00. Clauses 4.11 and 5.7.4.4 check a query only against the instances inside the window, so the
//! replays that print B9211 observe `brandName` at 12:00, when the window opens.

use andromeda_test_suite::{broker::BrokerClient, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::model::{DateTime, EntityId, Property, evolution::TemporalEntity};

/// The speed readings of B9211 and the instants they are observed at.
pub(crate) const SPEEDS: [(i64, &str); 3] = [(120, "2018-08-01T12:03:00Z"), (80, "2018-08-01T12:05:00Z"), (100, "2018-08-01T12:07:00Z")];

/// When B9211's `brandName` is observed, against the window from 12:00 to 13:00 the annex queries.
#[derive(Debug, Clone, Copy)]
pub(crate) enum BrandObserved {
    /// At 12:00, when the window opens, so a query over the window reads it.
    AtWindowStart,
    /// At 11:00, before the window, as the annex prints it.
    BeforeWindow,
}

impl BrandObserved {
    /// The instant `brandName` is observed at.
    const fn instant(self) -> &'static str {
        match self {
            Self::AtWindowStart => "2018-08-01T12:00:00Z",
            Self::BeforeWindow => "2018-08-01T11:00:00Z",
        }
    }
}

/// Records the evolution of B9211 through `client`, its `brandName` observed as `brand` says, and answers its id.
pub(crate) async fn record_b9211(client: &BrokerClient, namespace: &Namespace, brand: BrandObserved) -> Result<EntityId> {
    let mut evolution = TemporalEntity::builder()
        .id(namespace.urn("Vehicle", "B9211")?)
        .entity_type(namespace.entity_type("Vehicle")?)
        .property("brandName", Property::builder("Volvo").observed_at(brand.instant().parse::<DateTime>()?));
    for (speed, at) in SPEEDS {
        evolution = evolution.property("speed", Property::builder(speed).observed_at(at.parse::<DateTime>()?));
    }
    let evolution = evolution.build()?;
    client.temporal().create_or_update(&evolution).send().await?;
    Ok(evolution.id)
}
