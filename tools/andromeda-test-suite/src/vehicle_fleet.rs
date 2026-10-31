//! The fleets of vehicles the Snapshot load tests write before capturing or cloning them: thousands of Entities that
//! each carry one `reading`, and hundreds of Temporal Evolutions that each record [`OBSERVATIONS_PER_VEHICLE`] `speed`
//! instances, and the per-vehicle instance counts a branch answers for an observed fleet.
//!
//! Vehicle `index` of a fleet is `urn:ngsi-ld:<kind>:<token>-<label>-<index>`, the index written with four digits. An
//! observed vehicle in slot `s` records value `k` at `2025-01-01T00:00:00Z` plus `20 s + k` seconds, so the vehicles of
//! up to 4,320 slots never share an instant and all of them fall on 2025-01-01.

use crate::{
    broker::SuiteTransport,
    namespace::Namespace,
    outcome::{Done, Upserted},
};
use futures::{StreamExt, TryStreamExt, stream};
use googletest::prelude::*;
use ngsi_ld::{
    client::{Entities, Temporal},
    http::operation::RepresentationSpelling,
    model::{
        DateTime,
        Entity,
        EntityId,
        EntityTypeName,
        Input,
        Property,
        evolution::{AggregatedTemporalEntity, AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// The `speed` instances each observed vehicle records.
pub const OBSERVATIONS_PER_VEHICLE: u32 = 20;

/// The Entities one Batch Entity Creation of a fleet carries.
const BATCH_SIZE: usize = 100;

/// The Create Temporal Evolution requests of a fleet in flight at once.
const CONCURRENT_RECORDS: usize = 24;

/// The vehicles one page of [`observation_totals`] answers.
const PAGE_SIZE: u32 = 100;

/// The seconds in the day every observation falls on.
const SECONDS_PER_DAY: u32 = 86_400;

/// The id of vehicle `index` of the fleet `label` of `kind`.
///
/// # Errors
///
/// Returns the error of the namespace when `kind` or `label` breaks the URN.
pub fn vehicle_id(namespace: &Namespace, kind: &str, label: &str, index: u32) -> Result<EntityId> {
    Ok(namespace.urn(kind, format!("{label}-{index:04}"))?)
}

/// The `count` vehicles of the fleet `label`, of the namespace's type `kind`, vehicle `index` carrying the
/// `reading` `index`.
///
/// # Errors
///
/// Returns the error of the namespace or of the build when a name or an identifier is not valid.
pub fn current_vehicles(namespace: &Namespace, kind: &str, label: &str, count: u32) -> Result<Vec<Entity<Input>>> {
    let vehicle_type = namespace.entity_type(kind)?;
    (0..count)
        .map(|index| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(vehicle_id(namespace, kind, label, index)?)
                // Clone: every vehicle of the fleet owns its type.
                .entity_type(vehicle_type.clone())
                .property("reading", i64::from(index))
                .build()?)
        })
        .collect()
}

/// The `count` observed vehicles of the fleet `label`, of the namespace's type `kind`, vehicle `index` in slot
/// `first_slot + index`.
///
/// # Errors
///
/// Returns the error of the namespace or of the build when a name, an identifier or an instant is not valid, and a
/// failure when a slot's instants leave 2025-01-01.
pub fn observed_vehicles(namespace: &Namespace, kind: &str, label: &str, count: u32, first_slot: u32) -> Result<Vec<TemporalEntity<Input>>> {
    let vehicle_type = namespace.entity_type(kind)?;
    (0..count)
        .map(|index| -> Result<TemporalEntity<Input>> {
            let start = TemporalEntity::builder()
                .id(vehicle_id(namespace, kind, label, index)?)
                // Clone: every vehicle of the fleet owns its type.
                .entity_type(vehicle_type.clone());
            let slot = first_slot + index;
            let observed = (0..OBSERVATIONS_PER_VEHICLE).try_fold(start, |builder, value| -> Result<_> {
                Ok(builder.property("speed", Property::builder(i64::from(value)).observed_at(observed_at(slot, value)?)))
            })?;
            Ok(observed.build()?)
        })
        .collect()
}

/// The instant at which the vehicle in `slot` records the value `value`.
fn observed_at(slot: u32, value: u32) -> Result<DateTime> {
    let second = slot * OBSERVATIONS_PER_VEHICLE + value;
    verify_that!(second, lt(SECONDS_PER_DAY))?;
    Ok(format!("2025-01-01T{:02}:{:02}:{:02}Z", second / 3600, second % 3600 / 60, second % 60).parse()?)
}

/// Creates `vehicles` through `entities`, [`BATCH_SIZE`] to a Batch Entity Creation, each batch created whole.
///
/// # Errors
///
/// Returns the error of the broker, or the partial outcome of a batch it created only in part.
pub async fn create_current(entities: Entities<'_, SuiteTransport>, vehicles: &[Entity<Input>]) -> Result<()> {
    for batch in vehicles.chunks(BATCH_SIZE) {
        entities.batch_create(batch).send().await?.done()?;
    }
    Ok(())
}

/// Records `vehicles` through `temporal`, one Create Temporal Evolution each, [`CONCURRENT_RECORDS`] in flight at once.
///
/// # Errors
///
/// Returns the first error of the broker, or the first outcome that created no Temporal Evolution.
pub async fn record_observed(temporal: Temporal<'_, SuiteTransport>, vehicles: &[TemporalEntity<Input>]) -> Result<()> {
    stream::iter(vehicles)
        .map(|vehicle| record_one(&temporal, vehicle))
        .buffer_unordered(CONCURRENT_RECORDS)
        .try_collect::<Vec<()>>()
        .await?;
    Ok(())
}

/// Records `vehicle` through `temporal` with Create Temporal Evolution, which must create it.
async fn record_one(temporal: &Temporal<'_, SuiteTransport>, vehicle: &TemporalEntity<Input>) -> Result<()> {
    temporal.create_or_update(vehicle).send().await?.created()?;
    Ok(())
}

/// The `totalCount` of `speed` over 2025-01-01 of each of the `count` vehicles of `vehicle_type` that `temporal`
/// answers, read [`PAGE_SIZE`] vehicles to a page of Query Temporal Evolution in the aggregated representation.
///
/// # Errors
///
/// Returns the error of the broker or of a query argument.
pub async fn observation_totals(temporal: Temporal<'_, SuiteTransport>, vehicle_type: &EntityTypeName, count: u32) -> Result<Vec<AggregatedTemporalEntity>> {
    let day = ("2025-01-01T00:00:00Z".parse::<DateTime>()?, "2025-01-02T00:00:00Z".parse::<DateTime>()?);
    let mut totals = Vec::new();
    for offset in (0..count).step_by(usize::try_from(PAGE_SIZE)?) {
        let page = temporal
            .query_aggregated(
                temporal_query::between(day.0, day.1)
                    .aggregation_methods([AggregationMethod::TotalCount])
                    .aggregation_period("P1D".parse()?),
            )
            .entity_type(vehicle_type)
            .attrs(vec!["speed".parse()?])
            .spelling(RepresentationSpelling::default().in_options())
            .limit(PAGE_SIZE)
            .offset(offset)
            .send()
            .await?;
        totals.extend(page.into_items());
    }
    Ok(totals)
}

#[cfg(test)]
mod tests {
    use crate::{
        namespace::Namespace,
        vehicle_fleet::{OBSERVATIONS_PER_VEHICLE, current_vehicles, observed_at, observed_vehicles, vehicle_id},
    };
    use googletest::prelude::*;
    use ngsi_ld::model::DateTime;

    #[gtest]
    fn a_vehicle_id_carries_its_label_and_a_four_digit_index() -> Result<()> {
        let namespace = Namespace::fresh();
        verify_that!(
            vehicle_id(&namespace, "Vehicle", "local", 7)?.as_str(),
            eq(&format!("urn:ngsi-ld:Vehicle:{namespace}-local-0007"))
        )
    }

    #[gtest]
    fn a_current_fleet_holds_one_vehicle_per_index() -> Result<()> {
        let namespace = Namespace::fresh();
        let fleet = current_vehicles(&namespace, "Vehicle", "local", 3)?;
        expect_that!(fleet, len(eq(3)));
        verify_that!(
            fleet.last().map(|vehicle| &vehicle.id),
            some(eq(&vehicle_id(&namespace, "Vehicle", "local", 2)?))
        )
    }

    #[gtest]
    fn an_observed_vehicle_records_twenty_speeds() -> Result<()> {
        let namespace = Namespace::fresh();
        let fleet = observed_vehicles(&namespace, "Vehicle", "remote", 2, 500)?;
        expect_that!(fleet, len(eq(2)));
        verify_that!(fleet.first().map(|vehicle| vehicle.attributes.len()), some(eq(1)))
    }

    #[gtest]
    fn the_slots_of_a_thousand_vehicles_stay_inside_one_day() -> Result<()> {
        expect_that!(observed_at(0, 0)?.timestamp(), eq("2025-01-01T00:00:00Z".parse::<DateTime>()?.timestamp()));
        expect_that!(
            observed_at(999, OBSERVATIONS_PER_VEHICLE - 1)?.timestamp(),
            eq("2025-01-01T05:33:19Z".parse::<DateTime>()?.timestamp())
        );
        verify_that!(observed_at(4_320, 0), err(anything()))
    }
}
