//! The registrations the single-entity provision cases forward through: a Vehicle of the test named by its id, or
//! every Vehicle of a private tenant by its type, for one group of operations.

use crate::distributed_operations::forwarding_source::registration_id;
use andromeda_test_suite::namespace::Namespace;
use googletest::prelude::*;
use ngsi_ld::{
    model::{EntityTypeName, Input},
    registration::{CSourceRegistration, EntityInfo, OperationGroup, RegistrationInfo, RegistrationMode},
};
use url::Url;

/// What one registration covers of the Vehicle it names: the operations, and the attributes when it names any.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Coverage<'a> {
    /// The operation group the registration serves.
    pub(crate) operations: OperationGroup,
    /// The `propertyNames` of the registration; empty names every attribute.
    pub(crate) properties: &'a [&'a str],
}

/// The registration `urn:ngsi-ld:ContextSourceRegistration:<ns>-<suffix>` of the Vehicle `urn:ngsi-ld:Vehicle:<ns>-<n>`
/// at `endpoint` with `coverage`, in `mode` when it names one.
pub(crate) fn vehicle_id_registration(
    namespace: &Namespace,
    suffix: u32,
    n: u32,
    endpoint: &Url,
    mode: Option<RegistrationMode>,
    coverage: Coverage<'_>,
) -> Result<CSourceRegistration<Input>> {
    let vehicle = [EntityInfo::new("Vehicle".parse::<EntityTypeName>()?).with_id(namespace.urn("Vehicle", n)?)];
    let information = match coverage.properties {
        [] => RegistrationInfo::builder().entities(vehicle).build()?,
        names => RegistrationInfo::builder().entities(vehicle).property_names(names.iter().copied()).build()?,
    };
    Ok(CSourceRegistration::builder(endpoint.as_str())
        .id(registration_id(namespace, suffix)?)
        .information([information])
        .operations([coverage.operations])
        .maybe_mode(mode)
        .build()?)
}

/// The registration `urn:ngsi-ld:ContextSourceRegistration:<ns>-<suffix>` of every Vehicle at `endpoint` for the
/// redirection operations, in `mode` when it names one.
pub(crate) fn vehicle_type_registration(
    namespace: &Namespace,
    suffix: u32,
    endpoint: &Url,
    mode: Option<RegistrationMode>,
) -> Result<CSourceRegistration<Input>> {
    Ok(CSourceRegistration::builder(endpoint.as_str())
        .id(registration_id(namespace, suffix)?)
        .information([RegistrationInfo::builder().entities(["Vehicle"]).build()?])
        .operations([OperationGroup::RedirectionOps])
        .maybe_mode(mode)
        .build()?)
}

/// The redirection operations over every attribute of the Vehicle.
pub(crate) const REDIRECTION: Coverage<'static> = Coverage {
    operations: OperationGroup::RedirectionOps,
    properties: &[],
};
