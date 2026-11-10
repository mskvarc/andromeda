//! The fixtures of the bounded-query tests: Vehicles and their histories held on a peer broker, and the registration
//! of a peer at the broker that bounds a forwarded query (clauses 4.12, 5.7.2.4 and 5.7.4.4).

use crate::{
    broker::{self, BrokerClient},
    config::PeerRole,
    namespace::Namespace,
    outcome::{Done, Upserted},
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Input, Property, evolution::TemporalEntity},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationId, RegistrationInfo, RegistrationMode},
};

/// The Vehicles numbered `numbers`, each holding its number as `speed`.
///
/// # Errors
///
/// Returns the failure of building an identifier or an Entity.
pub fn vehicles(namespace: &Namespace, numbers: &[u32]) -> Result<Vec<Entity<Input>>> {
    let vehicle_type = namespace.entity_type("Vehicle")?;
    numbers
        .iter()
        .map(|&number| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Vehicle", number)?)
                .entity_type(vehicle_type.clone())
                .property("speed", number)
                .build()?)
        })
        .collect()
}

/// Makes `holder` hold the Vehicles numbered `numbers`.
///
/// # Errors
///
/// Returns the failure of building an Entity or of the batch create, and a partial success as one.
pub async fn hold_vehicles(holder: &BrokerClient, namespace: &Namespace, numbers: &[u32]) -> Result<()> {
    let entities = vehicles(namespace, numbers)?;
    holder.entities().batch_create(&entities).send().await?.done()?;
    Ok(())
}

/// Makes `holder` hold a history of the Vehicles numbered `numbers`, each one `speed` instance of its number observed
/// at the start of 2026.
///
/// # Errors
///
/// Returns the failure of building an identifier or an evolution, or of creating one.
pub async fn hold_vehicle_histories(holder: &BrokerClient, namespace: &Namespace, numbers: &[u32]) -> Result<()> {
    let vehicle_type = namespace.entity_type("Vehicle")?;
    for &number in numbers {
        let evolution = TemporalEntity::builder()
            .id(namespace.urn("Vehicle", number)?)
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(number).observed_at("2026-01-01T00:00:00Z".parse()?))
            .build()?;
        holder.temporal().create_or_update(&evolution).send().await?.created()?;
    }
    Ok(())
}

/// Registers `peer` at `bounded` as the source of the Vehicles of the namespace for Query Entities, in `mode`, under
/// the registration number `number`, and answers the identifier of the registration.
///
/// # Errors
///
/// Returns the failure of building or sending the registration.
pub async fn register_peer(bounded: &BrokerClient, namespace: &Namespace, number: u32, peer: PeerRole, mode: RegistrationMode) -> Result<RegistrationId> {
    register_peer_for(bounded, namespace, number, peer, mode, DistributedOperation::QueryEntity).await
}

/// Registers `peer` at `bounded` as the source of the Vehicles of the namespace for Query Temporal Evolution of
/// Entities, in `mode`, under the registration number `number`, and answers the identifier of the registration.
///
/// # Errors
///
/// Returns the failure of building or sending the registration.
pub async fn register_temporal_peer(
    bounded: &BrokerClient,
    namespace: &Namespace,
    number: u32,
    peer: PeerRole,
    mode: RegistrationMode,
) -> Result<RegistrationId> {
    register_peer_for(bounded, namespace, number, peer, mode, DistributedOperation::QueryTemporal).await
}

/// Registers `peer` at `bounded` as the source of the Vehicles of the namespace for `operation`, in `mode`, under the
/// registration number `number`, and answers the identifier of the registration.
async fn register_peer_for(
    bounded: &BrokerClient,
    namespace: &Namespace,
    number: u32,
    peer: PeerRole,
    mode: RegistrationMode,
    operation: DistributedOperation,
) -> Result<RegistrationId> {
    let id = namespace.registration_id(number)?;
    let registration = CSourceRegistration::builder(broker::peer(peer)?)
        .id(id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(namespace.entity_type("Vehicle")?)])
            .build()?])
        .mode(mode)
        .operations([operation])
        .build()?;
    bounded.registrations().register(&registration).send().await?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::vehicles;
    use crate::namespace::Namespace;
    use googletest::prelude::*;
    use ngsi_ld::model::OneOrMany;

    #[gtest]
    fn each_vehicle_carries_its_number_in_its_identifier_and_type() -> Result<()> {
        let namespace = Namespace::fresh();
        let held = vehicles(&namespace, &[3, 9])?;
        let vehicle_type = OneOrMany::one(namespace.entity_type("Vehicle")?);
        expect_that!(held, len(eq(2)));
        expect_that!(held.first().map(|entity| &entity.id), some(eq(&namespace.urn("Vehicle", 3)?)));
        expect_that!(held.last().map(|entity| &entity.id), some(eq(&namespace.urn("Vehicle", 9)?)));
        expect_that!(held.first().map(|entity| &entity.entity_type), some(eq(&vehicle_type)));
        Ok(())
    }
}
