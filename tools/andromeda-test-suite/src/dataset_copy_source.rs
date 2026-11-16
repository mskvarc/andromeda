//! A context source that accepts subscription copies and whose registration lists given `datasetId`s (Table 5.2.9-1),
//! so a test reads which sources a subscription naming `datasetId`s is copied to (clause 5.12).

use crate::{broker::BrokerClient, context_source::ContextSource, namespace::Namespace, relayed_copy::accept_copies};
use googletest::prelude::*;
use ngsi_ld::{
    model::DatasetId,
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};

/// Makes the source, beneath its root path `prefix`, accept copies and registers it with the broker as an inclusive
/// `federationOps` source for the test's `Vehicle` type and identifiers under the test's registration number
/// `number`, listing `dataset_ids` as its `datasetId` when there are any.
///
/// # Errors
///
/// Returns the failure to mount an answer, to build the registration or to register the source.
pub async fn register_dataset_copy_source(
    broker: &BrokerClient,
    source: &ContextSource,
    namespace: &Namespace,
    prefix: &str,
    number: u32,
    dataset_ids: Option<Vec<DatasetId>>,
) -> Result<()> {
    accept_copies(source, prefix).await?;
    let vehicles = EntityInfo::new(namespace.entity_type("Vehicle")?).with_id_pattern(namespace.id_pattern("Vehicle")?);
    let registration = CSourceRegistration::builder(source.endpoint(prefix)?)
        .id(namespace.registration_id(number)?)
        .information([RegistrationInfo::builder().entities([vehicles]).build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([OperationGroup::FederationOps]);
    let registration = match dataset_ids {
        Some(dataset_ids) => registration.dataset_ids(dataset_ids).build()?,
        None => registration.build()?,
    };
    broker.registrations().register(&registration).send().await?;
    Ok(())
}
