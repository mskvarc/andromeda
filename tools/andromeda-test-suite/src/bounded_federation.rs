//! The fixtures of the bounded-query tests: Vehicles held on a peer broker, and the registration of a peer at the
//! broker that bounds a forwarded query (clauses 4.12 and 5.7.2.4).

use crate::{
    broker::{self, BrokerClient},
    config::PeerRole,
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    model::{Entity, Input},
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

/// Registers `peer` at `bounded` as the source of the Vehicles of the namespace for Query Entities, in `mode`, under
/// the registration number `number`, and answers the identifier of the registration.
///
/// # Errors
///
/// Returns the failure of building or sending the registration.
pub async fn register_peer(bounded: &BrokerClient, namespace: &Namespace, number: u32, peer: PeerRole, mode: RegistrationMode) -> Result<RegistrationId> {
    let id = namespace.registration_id(number)?;
    let registration = CSourceRegistration::builder(broker::peer(peer)?)
        .id(id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(namespace.entity_type("Vehicle")?)])
            .build()?])
        .mode(mode)
        .operations([DistributedOperation::QueryEntity])
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
