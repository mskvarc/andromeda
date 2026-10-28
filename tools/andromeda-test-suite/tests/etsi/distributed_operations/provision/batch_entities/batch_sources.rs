//! The registrations and the teardown the batch cases share: the Vehicles `urn:ngsi-ld:Vehicle:<ns>-1` and `-2` of a
//! test registered for the redirection operations and every batch operation, inclusively, exclusively or for redirect.

use crate::distributed_operations::forwarding_source::registration_id;
use andromeda_test_suite::{broker::BrokerClient, namespace::Namespace, teardown::AnyAnswer};
use googletest::prelude::*;
use ngsi_ld::{
    model::{EntityTypeName, Input},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode, SupportedOperation},
};
use url::Url;

/// The operations every batch registration names: the redirection operations and the five batch operations.
const fn batch_operations() -> [SupportedOperation; 6] {
    [
        SupportedOperation::Group(OperationGroup::RedirectionOps),
        SupportedOperation::Operation(DistributedOperation::CreateBatch),
        SupportedOperation::Operation(DistributedOperation::UpsertBatch),
        SupportedOperation::Operation(DistributedOperation::UpdateBatch),
        SupportedOperation::Operation(DistributedOperation::DeleteBatch),
        SupportedOperation::Operation(DistributedOperation::MergeBatch),
    ]
}

/// The registration `urn:ngsi-ld:ContextSourceRegistration:<ns>-<suffix>` of every Vehicle of the namespace by its
/// `idPattern`, at `endpoint`, in `mode` when it names one.
pub(crate) fn batch_registration(namespace: &Namespace, suffix: u32, endpoint: &Url, mode: Option<RegistrationMode>) -> Result<CSourceRegistration<Input>> {
    let vehicles = EntityInfo::new("Vehicle".parse::<EntityTypeName>()?).with_id_pattern(namespace.id_pattern("Vehicle")?);
    Ok(CSourceRegistration::builder(endpoint.as_str())
        .id(registration_id(namespace, suffix)?)
        .information([RegistrationInfo::builder().entities([vehicles]).build()?])
        .operations(batch_operations())
        .maybe_mode(mode)
        .build()?)
}

/// The registration `urn:ngsi-ld:ContextSourceRegistration:<ns>-1` of the `speed` of the Vehicles `-1` and `-2`, by id,
/// in the exclusive mode at `endpoint`.
pub(crate) fn exclusive_batch_registration(namespace: &Namespace, endpoint: &Url) -> Result<CSourceRegistration<Input>> {
    let vehicle = |n: u32| -> Result<EntityInfo> { Ok(EntityInfo::new("Vehicle".parse::<EntityTypeName>()?).with_id(namespace.urn("Vehicle", n)?)) };
    Ok(CSourceRegistration::builder(endpoint.as_str())
        .id(registration_id(namespace, 1)?)
        .information([RegistrationInfo::builder()
            .entities([vehicle(1)?, vehicle(2)?])
            .property_names(["speed"])
            .build()?])
        .operations(batch_operations())
        .mode(RegistrationMode::Exclusive)
        .build()?)
}

/// The batch teardown: `Batch Entity Delete` of the Vehicles `-1` and `-2`, whose answer is ignored.
pub(crate) async fn delete_vehicles(broker: &BrokerClient, namespace: &Namespace) -> Result<()> {
    broker
        .entities()
        .batch_delete(&[namespace.urn("Vehicle", 1)?, namespace.urn("Vehicle", 2)?])
        .send()
        .await
        .any_answer();
    Ok(())
}
