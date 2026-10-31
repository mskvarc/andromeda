//! The Context Source Registrations and information elements the context source conformance tests register, and the
//! terms of the `@context` those tests register.

use andromeda_test_suite::namespace::Namespace;
use googletest::Result;
use ngsi_ld::{
    model::{DateTime, EntityId, EntityTypeName, Input},
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, RegistrationId, RegistrationInfo},
};
use url::Url;

/// The terms of the `@context` the context source tests register, as `vehicle_parking_registration` names them.
pub(crate) const VEHICLE_PARKING_TERMS: [&str; 8] = [
    "Vehicle",
    "brandName",
    "speed",
    "isParked",
    "OffStreetParking",
    "availableSpotNumber",
    "totalSpotNumber",
    "isNextToBuilding",
];

/// The terms of the `@context` the context source discovery cases register: [`VEHICLE_PARKING_TERMS`], then those of
/// [`building_registration`].
pub(crate) const VEHICLE_PARKING_BUILDING_TERMS: [&str; 12] = [
    "Vehicle",
    "brandName",
    "speed",
    "isParked",
    "OffStreetParking",
    "availableSpotNumber",
    "totalSpotNumber",
    "isNextToBuilding",
    "Building",
    "name",
    "subCategory",
    "locatedAt",
];

/// The context source registration the context source tests set up: the Vehicle `vehicle` with `brandName`, `speed` and
/// `isParked`, and every `OffStreetParking` of the namespace by pattern, with `availableSpotNumber`, `totalSpotNumber`
/// and `isNextToBuilding`, at `endpoint`, expiring at `expires_at` when it names one.
pub(crate) fn vehicle_parking_registration(
    namespace: &Namespace,
    endpoint: &Url,
    vehicle: EntityId,
    expires_at: Option<DateTime>,
) -> Result<CSourceRegistration<Input>> {
    let id: RegistrationId = namespace.urn_text("ContextSourceRegistration", 1).parse()?;
    Ok(CSourceRegistration::builder(endpoint.as_str())
        .id(id)
        .information([a456_information(vehicle)?, parking_information(namespace)?])
        .maybe_expires_at(expires_at)
        .build()?)
}

/// The registration `urn:ngsi-ld:ContextSourceRegistration:<ns>-<suffix>` of every `Building`, with `name`,
/// `subCategory` and `locatedAt`, at `endpoint`.
pub(crate) fn building_registration(namespace: &Namespace, suffix: u32, endpoint: &Url) -> Result<CSourceRegistration<Input>> {
    Ok(CSourceRegistration::builder(endpoint.as_str())
        .id(namespace.urn_text("ContextSourceRegistration", suffix).parse::<RegistrationId>()?)
        .information([RegistrationInfo::builder()
            .entities(["Building"])
            .property_names(["name", "subCategory"])
            .relationship_names(["locatedAt"])
            .build()?])
        .build()?)
}

/// The information element naming the Vehicle `vehicle`: with `brandName`, `speed` and `isParked`.
pub(crate) fn a456_information(vehicle: EntityId) -> Result<RegistrationInfo> {
    Ok(RegistrationInfo::builder()
        .entities([EntityInfo::new("Vehicle".parse::<EntityTypeName>()?).with_id(vehicle)])
        .property_names(["brandName", "speed"])
        .relationship_names(["isParked"])
        .build()?)
}

/// The information element naming every `OffStreetParking` of the namespace by pattern, with `availableSpotNumber`,
/// `totalSpotNumber` and `isNextToBuilding`.
pub(crate) fn parking_information(namespace: &Namespace) -> Result<RegistrationInfo> {
    Ok(RegistrationInfo::builder()
        .entities([
            EntityInfo::new("OffStreetParking".parse::<EntityTypeName>()?).with_id_pattern(offstreet_pattern(namespace, ".*downtown$")?),
            EntityInfo::new("OffStreetParking".parse::<EntityTypeName>()?).with_id_pattern(offstreet_pattern(namespace, ".*47$")?),
        ])
        .property_names(["availableSpotNumber", "totalSpotNumber"])
        .relationship_names(["isNextToBuilding"])
        .build()?)
}

/// The pattern `urn:ngsi-ld:OffStreetParking:<ns>-<tail>`: the case's pattern, prefixed with the test's namespace so
/// it matches only the test's own entities.
pub(crate) fn offstreet_pattern(namespace: &Namespace, tail: &str) -> Result<IdPattern> {
    Ok(format!("urn:ngsi-ld:OffStreetParking:{}-{tail}", namespace.token()).parse()?)
}
