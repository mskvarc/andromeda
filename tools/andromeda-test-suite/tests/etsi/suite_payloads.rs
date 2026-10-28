//! The payload files of the ETSI suite that several cases load, built with the client's builders and namespaced for
//! the test that loads them.

use andromeda_test_suite::namespace::Namespace;
use cefact_units::UnitCode;
use googletest::Result;
use ngsi_ld::{
    http::UncheckedDocument,
    model::{
        Attribute,
        AttributeDraft,
        AttributeName,
        DatasetId,
        DateTime,
        Entity,
        EntityFragment,
        EntityId,
        EntityTypeName,
        GeoProperty,
        Input,
        JsonProperty,
        LanguageMap,
        LanguageProperty,
        Lossless,
        OneOrMany,
        Patch,
        Property,
        RawJson,
        Relationship,
        Representation,
        Scope,
        ScopeChange,
        Value,
        VocabProperty,
        VocabTerm,
        evolution::{TemporalAttributes, TemporalEntity},
        geometry::{LinearRing, Polygon, Position, Shape},
    },
    query::{EntitySelector, IdPattern},
    registration::{CSourceRegistration, EntityInfo, RegistrationId, RegistrationInfo},
    subscription::{
        Activation,
        Endpoint,
        NotificationParams,
        RegistrationSubscription,
        Subscription,
        SubscriptionId,
        SubscriptionKind,
        SystemAttributeReporting,
    },
};
use std::fmt::Display;
use url::Url;

/// The terms of the `@context` the simple Building cases register, as `building_simple_attributes` names them.
pub(crate) const BUILDING_SIMPLE_TERMS: [&str; 5] = ["Building", "name", "subCategory", "airQualityLevel", "almostFull"];

/// The terms of the `@context` the replace cases register, as `pisa_tower_located_at_pisa` names them.
pub(crate) const BUILDING_REPLACE_TERMS: [&str; 6] = ["Building", "name", "subCategory", "airQualityLevel", "almostFull", "locatedAt"];

/// The terms of the `@context` the Building and Vehicle cases register, as `create_eiffel_vehicle_pisa` names them.
pub(crate) const BUILDING_VEHICLE_TERMS: [&str; 10] = [
    "Building",
    "name",
    "subCategory",
    "airQualityLevel",
    "almostFull",
    "Vehicle",
    "brandName",
    "isParked",
    "providedBy",
    "locatedIn",
];

/// The terms of the `@context` the temporal Vehicle cases register, as `vehicle_speed_fuel_level` names them.
pub(crate) const VEHICLE_SPEED_FUEL_TERMS: [&str; 3] = ["Vehicle", "speed", "fuelLevel"];

/// The terms of the `@context` the add-attributes cases register: [`VEHICLE_SPEED_FUEL_TERMS`] and the attribute
/// `added_attribute` that they add.
pub(crate) const VEHICLE_SPEED_FUEL_ADDED_TERMS: [&str; 4] = ["Vehicle", "speed", "fuelLevel", "added_attribute"];

/// The terms of the `@context` the replace cases register, as `vehicle_parked_speed_sources` names them.
pub(crate) const VEHICLE_PARKED_SPEED_SOURCE_TERMS: [&str; 5] = ["Vehicle", "isParked", "providedBy", "speed", "source"];

/// The terms of the `@context` the linked-entity cases register for a Building located at a City in a Country.
pub(crate) const PARIS_IN_FRANCE_TERMS: [&str; 6] = ["Building", "locatedAt", "City", "name", "isInCountry", "Country"];

/// The terms of the `@context` the Merge Entity cases on a Building that holds a name, a `locatedAt` and a `street`.
pub(crate) const BUILDING_LOCATED_STREET_TERMS: [&str; 4] = ["Building", "name", "locatedAt", "street"];

/// The terms of the `@context` the partial-update cases on a Building with a `jsonProperty` register.
pub(crate) const BUILDING_JSON_PROPERTY_TERMS: [&str; 4] = ["Building", "jsonProperty", "source", "providedBy"];

/// The terms of the `@context` the partial-update cases on a Building with a `vocabProperty` register.
pub(crate) const BUILDING_VOCAB_PROPERTY_TERMS: [&str; 4] = ["Building", "vocabProperty", "source", "providedBy"];

/// The terms of the `@context` the sub-attribute removal cases on a Building with an `airQualityLevel` that holds a
/// `hasWeatherObserved` Relationship register.
pub(crate) const BUILDING_AIR_QUALITY_WEATHER_TERMS: [&str; 6] = ["Building", "name", "subCategory", "airQualityLevel", "hasWeatherObserved", "almostFull"];

/// The terms of the `@context` the linked cases with a street and a location register, as `create_street_linked_pairs`
/// names them.
pub(crate) const STREET_LINKED_TERMS: [&str; 6] = ["Building", "name", "locatedAt", "street", "City", "isInCountry"];

/// The terms of the `@context` the context source cases register, as `vehicle_parking_registration` names them.
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

/// `entities/building-simple-attributes.jsonld`, as `urn:ngsi-ld:Building:<ns>-<suffix>`.
pub(crate) fn building_simple_attributes(namespace: &Namespace, suffix: impl Display) -> Result<Entity<Input>> {
    building_with_air_quality(
        namespace,
        suffix,
        Property::builder(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00.000Z".parse()?),
    )
}

/// The Building `urn:ngsi-ld:Building:<ns>-<suffix>` named Eiffel Tower, sub-categorised as tourism, with the
/// `airQualityLevel` that `air_quality` builds and an `almostFull` of false.
pub(crate) fn building_with_air_quality(
    namespace: &Namespace,
    suffix: impl Display,
    air_quality: impl AttributeDraft<Property<Input>>,
) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", suffix)?)
        .entity_type("Building")
        .property("name", "Eiffel Tower")
        .property("subCategory", "tourism")
        .property("airQualityLevel", air_quality)
        .property("almostFull", Value::Boolean(false))
        .build()?)
}

/// The Building `urn:ngsi-ld:Building:<ns>-<suffix>` named Eiffel Tower, whose `airQualityLevel` of 4 C62 observed at
/// 2020-09-09T16:40:00.000Z has a `hasWeatherObserved` Relationship to the Sensor
/// `urn:ngsi-ld:Sensor:<ns>-43a1af27-4aa7-44fe-bee9-fe76d56978a4`.
pub(crate) fn building_with_weather_sensor(namespace: &Namespace, suffix: impl Display) -> Result<Entity<Input>> {
    let weather = Relationship::builder(namespace.urn("Sensor", "43a1af27-4aa7-44fe-bee9-fe76d56978a4")?);
    let air_quality = Property::builder(4)
        .unit_code(UnitCode::C62)
        .observed_at("2020-09-09T16:40:00.000Z".parse()?)
        .relationship("hasWeatherObserved", weather);
    building_with_air_quality(namespace, suffix, air_quality)
}

/// The Building `urn:ngsi-ld:Building:<ns>-<suffix>` named Pisa Tower, sub-categorised as tourism, with an
/// `airQualityLevel` of 6 observed at 2020-10-10T16:40:00.000Z and an `almostFull` of true.
pub(crate) fn pisa_tower(namespace: &Namespace, suffix: impl Display) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", suffix)?)
        .entity_type("Building")
        .property("name", "Pisa Tower")
        .property("subCategory", "tourism")
        .property(
            "airQualityLevel",
            Property::builder(6).unit_code(UnitCode::C62).observed_at("2020-10-10T16:40:00.000Z".parse()?),
        )
        .property("almostFull", Value::Boolean(true))
        .build()?)
}

/// The Building `urn:ngsi-ld:Building:<ns>-3` named Pisa Tower, sub-categorised as sightseeing, with an
/// `airQualityLevel` of 7 observed at 2023-10-10T16:40:00.000Z, an `almostFull` of false and a `locatedIn`
/// Relationship to `urn:ngsi-ld:City:<ns>-Pisa`.
pub(crate) fn pisa_tower_sightseeing(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 3)?)
        .entity_type("Building")
        .property("name", "Pisa Tower")
        .property("subCategory", "sightseeing")
        .property(
            "airQualityLevel",
            Property::builder(7).unit_code(UnitCode::C62).observed_at("2023-10-10T16:40:00.000Z".parse()?),
        )
        .property("almostFull", Value::Boolean(false))
        .relationship("locatedIn", Relationship::builder(namespace.urn("City", "Pisa")?))
        .build()?)
}

/// The Building `urn:ngsi-ld:Building:<ns>-<suffix>` named Pisa Tower, with a `locatedAt` Relationship to the City
/// `urn:ngsi-ld:City:<ns>-Pisa`: the representation the replace cases send.
pub(crate) fn pisa_tower_located_at_pisa(namespace: &Namespace, suffix: impl Display) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", suffix)?)
        .entity_type("Building")
        .property("name", "Pisa Tower")
        .relationship("locatedAt", Relationship::builder(namespace.urn("City", "Pisa")?))
        .build()?)
}

/// The Building `urn:ngsi-ld:Building:<ns>-1` whose `scope` is the single scope `/Madrid/Gardens/ParqueNorte`.
pub(crate) fn building_with_one_scope(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .scope("/Madrid/Gardens/ParqueNorte")
        .build()?)
}

/// The Building `urn:ngsi-ld:Building:<ns>-1` whose `scope` lists `/Madrid/Gardens/ParqueNorte` and
/// `/CompanyA/OrganizationB/UnitC`.
pub(crate) fn building_with_many_scopes(namespace: &Namespace) -> Result<Entity<Input>> {
    let scopes = vec![
        "/Madrid/Gardens/ParqueNorte".parse::<Scope>()?,
        "/CompanyA/OrganizationB/UnitC".parse::<Scope>()?,
    ];
    Ok(Entity::builder()
        .id(namespace.urn("Building", 1)?)
        .entity_type("Building")
        .scope(OneOrMany::try_from(scopes)?)
        .build()?)
}

/// The `isParked` Relationship of the suite's vehicle parked downtown: the `OffStreetParking` Downtown1, observed at
/// 2017-07-29T12:00:04Z, provided by Bob.
pub(crate) fn is_parked_downtown1(namespace: &Namespace) -> Result<Relationship<Input>> {
    Ok(Relationship::builder(namespace.urn("OffStreetParking", "Downtown1")?)
        .observed_at("2017-07-29T12:00:04Z".parse()?)
        .relationship("providedBy", namespace.urn("Person", "Bob")?)
        .build()?)
}

/// The `isParked` Relationship the replace cases write: the `OffStreetParking` Downtown2, observed at
/// 2017-07-29T15:00:04Z, provided by Bob.
pub(crate) fn is_parked_downtown2(namespace: &Namespace) -> Result<Relationship<Input>> {
    Ok(Relationship::builder(namespace.urn("OffStreetParking", "Downtown2")?)
        .observed_at("2017-07-29T15:00:04Z".parse()?)
        .relationship("providedBy", namespace.urn("Person", "Bob")?)
        .build()?)
}

/// The suite's vehicle parked downtown, `urn:ngsi-ld:Vehicle:<ns>-<n>`: a Mercedes whose `isParked` Relationship is
/// [`is_parked_downtown1`].
pub(crate) fn vehicle_parked(namespace: &Namespace, suffix: impl Display) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", suffix)?)
        .entity_type("Vehicle")
        .property("brandName", "Mercedes")
        .relationship("isParked", is_parked_downtown1(namespace)?)
        .build()?)
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-021-26` of the multi-valued relationship cases: an `isParkedIn` Relationship
/// whose objects are the `OffStreetParking` entities Downtown1 and Downtown2, observed on 2020-08-01 at 12:03 in UTC.
pub(crate) fn vehicle_parked_in_two(namespace: &Namespace) -> Result<TemporalEntity<Input>> {
    Ok(TemporalEntity::builder()
        .id(namespace.urn("Vehicle", "021-26")?)
        .entity_type("Vehicle")
        .relationship(
            "isParkedIn",
            Relationship::builder(vec![
                namespace.urn("OffStreetParking", "Downtown1")?,
                namespace.urn("OffStreetParking", "Downtown2")?,
            ])
            .observed_at("2020-08-01T12:03:00Z".parse()?),
        )
        .build()?)
}

/// The Building `urn:ngsi-ld:Building:<ns>-<suffix>` with no attribute.
pub(crate) fn bare_building(namespace: &Namespace, suffix: impl Display) -> Result<Entity<Input>> {
    Ok(Entity::builder().id(namespace.urn("Building", suffix)?).entity_type("Building").build()?)
}

/// The subscription `id` to every Building, notifying `endpoint` in `format`, with no member beyond the entity
/// selector and the notification.
pub(crate) fn building_subscription(id: SubscriptionId, endpoint: Endpoint, format: Representation) -> Result<Subscription<Input>> {
    Ok(Subscription::builder()
        .id(id)
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(NotificationParams::builder(endpoint).format(format).build()?)
        .build()?)
}

/// The `notification` member written as the ETSI cases write it: the `keyValues` spelling of the format, and an
/// endpoint at `uri` that accepts `application/json`.
pub(crate) fn key_values_notification(uri: &Url) -> Result<RawJson> {
    key_values_notification_reporting(uri, SystemAttributeReporting::Unspecified)
}

/// The keyValues notification to `uri` with the `sysAttrs` member `reporting` writes, and none for
/// [`SystemAttributeReporting::Unspecified`].
pub(crate) fn key_values_notification_reporting(uri: &Url, reporting: SystemAttributeReporting) -> Result<RawJson> {
    let sys_attrs = match reporting {
        SystemAttributeReporting::Unspecified => "",
        SystemAttributeReporting::Include => r#","sysAttrs":true"#,
        SystemAttributeReporting::Omit => r#","sysAttrs":false"#,
    };
    Ok(format!(r#"{{"format":"keyValues","endpoint":{{"uri":"{uri}","accept":"application/json"}}{sys_attrs}}}"#).parse()?)
}

/// The document of `subscription` with its notification written as [`key_values_notification_reporting`] writes it.
pub(crate) fn key_values_document_reporting(subscription: &Subscription<Input>, uri: &Url, reporting: SystemAttributeReporting) -> Result<UncheckedDocument> {
    Ok(UncheckedDocument::try_from(subscription.render())?.set_member("notification", &key_values_notification_reporting(uri, reporting)?))
}

/// The document of `subscription` with its notification written as the ETSI cases write it, as
/// [`key_values_notification`] writes it.
pub(crate) fn key_values_document<K: SubscriptionKind>(subscription: &Subscription<Input, K>, uri: &Url) -> Result<UncheckedDocument> {
    Ok(UncheckedDocument::try_from(subscription.render())?.set_member("notification", &key_values_notification(uri)?))
}

/// The Context Source Registration Subscription to every Building, named `id` when it is given, notifying `uri` (the
/// notification [`key_values_document`] writes in its place), in the state `activation` writes and expiring at
/// `expires_at` when it names an instant.
pub(crate) fn building_registration_subscription(
    id: Option<SubscriptionId>,
    uri: &Url,
    activation: Activation,
    expires_at: Option<DateTime>,
) -> Result<Subscription<Input, RegistrationSubscription>> {
    Ok(Subscription::registration_builder()
        .maybe_id(id)
        .entities([EntitySelector::new("Building".parse()?)])
        .notification(NotificationParams::builder(Endpoint::builder(uri.as_str()).build()?).build()?)
        .activation(activation)
        .maybe_expires_at(expires_at)
        .build()?)
}

/// The entity `urn:ngsi-ld:Building:<ns>-<suffix>` with no attribute, of the types `types`.
pub(crate) fn building_of_types<const N: usize>(namespace: &Namespace, suffix: impl Display, types: [&str; N]) -> Result<Entity<Input>> {
    Ok(Entity::builder().id(namespace.urn("Building", suffix)?).entity_type(types).build()?)
}

/// A point at `longitude` and `latitude`, as a geometry value.
pub(crate) fn point(longitude: f64, latitude: f64) -> Result<Shape> {
    Ok(Shape::Point(Position::try_new(longitude, latitude)?))
}

/// The Building `urn:ngsi-ld:Building:<ns>-<suffix>` named Eiffel Tower and sub-categorised as tourism, located at the
/// point 13.3986, 52.5547.
pub(crate) fn eiffel_tower_located(namespace: &Namespace, suffix: impl Display) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", suffix)?)
        .entity_type("Building")
        .property("name", "Eiffel Tower")
        .property("subCategory", "tourism")
        .geo_property("location", point(13.3986, 52.5547)?)
        .build()?)
}

/// The `OffStreetParking` `urn:ngsi-ld:OffStreetParking:<ns>-<suffix>`: named Downtown One, with 121 available spots
/// observed at 2017-07-29T12:05:02Z (a reliability of 0.7, provided by the camera `urn:ngsi-ld:Camera:<ns>-C1`), 200
/// total spots, and located at the point -8.5, 41.2.
pub(crate) fn offstreet_parking(namespace: &Namespace, suffix: impl Display) -> Result<Entity<Input>> {
    let available = Property::builder(121)
        .observed_at("2017-07-29T12:05:02Z".parse()?)
        .property("reliability", Property::builder(Value::try_from(0.7)?))
        .relationship("providedBy", Relationship::builder(namespace.urn("Camera", "C1")?));
    Ok(Entity::builder()
        .id(namespace.urn("OffStreetParking", suffix)?)
        .entity_type("OffStreetParking")
        .property("name", "Downtown One")
        .property("availableSpotNumber", available)
        .property("totalSpotNumber", Property::builder(200))
        .geo_property("location", point(-8.5, 41.2)?)
        .build()?)
}

/// The context source registration the context source cases set up: the Vehicle `vehicle` with `brandName`, `speed`
/// and `isParked`, and every `OffStreetParking` of the namespace by pattern, with `availableSpotNumber`,
/// `totalSpotNumber` and `isNextToBuilding`, at `endpoint`, expiring at `expires_at` when it names one.
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

/// The Building of the linked-entity cases, `urn:ngsi-ld:Building:<ns>-<suffix>`, with a `locatedAt` Relationship to
/// [`city_paris`].
pub(crate) fn building_located_at(namespace: &Namespace, suffix: &str) -> Result<Entity<Input>> {
    let located_at = Relationship::builder(namespace.urn("City", "Paris")?).object_type("City".parse::<EntityTypeName>()?);
    Ok(Entity::builder()
        .id(namespace.urn("Building", suffix)?)
        .entity_type("Building")
        .relationship("locatedAt", located_at)
        .build()?)
}

/// The Building `urn:ngsi-ld:Building:<ns>-<suffix>` with a `locatedAt` Relationship to the City
/// `urn:ngsi-ld:City:<ns>-<suffix>`, whose `objectType` is `City`.
pub(crate) fn building_located_at_city(namespace: &Namespace, suffix: &str) -> Result<Entity<Input>> {
    let located_at = Relationship::builder(namespace.urn("City", suffix)?).object_type("City".parse::<EntityTypeName>()?);
    Ok(Entity::builder()
        .id(namespace.urn("Building", suffix)?)
        .entity_type("Building")
        .relationship("locatedAt", located_at)
        .build()?)
}

/// The City `urn:ngsi-ld:City:<ns>-<suffix>`, with no attribute.
pub(crate) fn city(namespace: &Namespace, suffix: &str) -> Result<Entity<Input>> {
    Ok(Entity::builder().id(namespace.urn("City", suffix)?).entity_type("City").build()?)
}

/// The City `urn:ngsi-ld:City:<ns>-<suffix>` with a `locatedAt` Relationship to the Country
/// `urn:ngsi-ld:Country:<ns>-<suffix>`, whose `objectType` is `Country`.
pub(crate) fn city_located_in_country(namespace: &Namespace, suffix: &str) -> Result<Entity<Input>> {
    let located_at = Relationship::builder(namespace.urn("Country", suffix)?).object_type("Country".parse::<EntityTypeName>()?);
    Ok(Entity::builder()
        .id(namespace.urn("City", suffix)?)
        .entity_type("City")
        .relationship("locatedAt", located_at)
        .build()?)
}

/// The Country `urn:ngsi-ld:Country:<ns>-<suffix>`, with no attribute.
pub(crate) fn country(namespace: &Namespace, suffix: &str) -> Result<Entity<Input>> {
    Ok(Entity::builder().id(namespace.urn("Country", suffix)?).entity_type("Country").build()?)
}

/// The City the linked-entity cases point at, `urn:ngsi-ld:City:<ns>-Paris`, with no attribute.
pub(crate) fn city_paris(namespace: &Namespace) -> Result<Entity<Input>> {
    city(namespace, "Paris")
}

/// The Building `urn:ngsi-ld:Building:<ns>-<suffix>` of the Paris cases, with a `locatedAt` Relationship to
/// [`city_paris_in_france`] and no `objectType`.
pub(crate) fn building_located_in_paris(namespace: &Namespace, suffix: &str) -> Result<Entity<Input>> {
    building_located_in_city(namespace, suffix, "Paris")
}

/// The Building `urn:ngsi-ld:Building:<ns>-<suffix>` with a `locatedAt` Relationship to the City
/// `urn:ngsi-ld:City:<ns>-<city>`, and no `objectType`.
pub(crate) fn building_located_in_city(namespace: &Namespace, suffix: &str, city: &str) -> Result<Entity<Input>> {
    let located_at = Relationship::builder(namespace.urn("City", city)?);
    Ok(Entity::builder()
        .id(namespace.urn("Building", suffix)?)
        .entity_type("Building")
        .relationship("locatedAt", located_at)
        .build()?)
}

/// The City `urn:ngsi-ld:City:<ns>-Paris` of the Paris cases: named and described, located in France.
pub(crate) fn city_paris_in_france(namespace: &Namespace) -> Result<Entity<Input>> {
    city_paris_in_country(namespace, "Paris", "France")
}

/// The City `urn:ngsi-ld:City:<ns>-<suffix>` named Paris and described as the capital of France, with an `isInCountry`
/// Relationship to the Country `urn:ngsi-ld:Country:<ns>-<country>`.
pub(crate) fn city_paris_in_country(namespace: &Namespace, suffix: &str, country: &str) -> Result<Entity<Input>> {
    let is_in_country = Relationship::builder(namespace.urn("Country", country)?);
    Ok(Entity::builder()
        .id(namespace.urn("City", suffix)?)
        .entity_type("City")
        .property("name", "Paris")
        .property("description", "Paris is the capital and largest city of France")
        .relationship("isInCountry", is_in_country)
        .build()?)
}

/// The Country `urn:ngsi-ld:Country:<ns>-France` of the Paris cases: named and described.
pub(crate) fn country_france_described(namespace: &Namespace) -> Result<Entity<Input>> {
    country_described(namespace, "France")
}

/// The Country `urn:ngsi-ld:Country:<ns>-<suffix>` named France and described as a European country.
pub(crate) fn country_described(namespace: &Namespace, suffix: &str) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Country", suffix)?)
        .entity_type("Country")
        .property("name", "France")
        .property("description", "France is an European country")
        .build()?)
}

/// The Building `urn:ngsi-ld:Building:<ns>-<suffix>` named Eiffel Tower, located at the point -8.5, 41.2, with a
/// `street` in French and Dutch and a `locatedAt` Relationship to the City `urn:ngsi-ld:City:<ns>-<suffix>`.
pub(crate) fn building_street_located(namespace: &Namespace, suffix: &str) -> Result<Entity<Input>> {
    let located_at = Relationship::builder(namespace.urn("City", suffix)?);
    Ok(Entity::builder()
        .id(namespace.urn("Building", suffix)?)
        .entity_type("Building")
        .property("name", "Eiffel Tower")
        .relationship("locatedAt", located_at)
        .geo_property("location", point(-8.5, 41.2)?)
        .language_property("street", [("fr", "Grand Place"), ("nl", "Grote Markt")])
        .build()?)
}

/// The Building `id` named Eiffel Tower, located at the City `urn:ngsi-ld:City:<ns>-Paris` by a `locatedAt`
/// Relationship, at the point -8.5, 41.2, with a `street` in French and Dutch.
pub(crate) fn building_located_street_geo(namespace: &Namespace, id: EntityId) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(id)
        .entity_type("Building")
        .property("name", "Eiffel Tower")
        .relationship("locatedAt", Relationship::builder(namespace.urn("City", "Paris")?))
        .geo_property("location", point(-8.5, 41.2)?)
        .language_property("street", [("fr", "Grand Place"), ("nl", "Grote Markt")])
        .build()?)
}

/// The Building `urn:ngsi-ld:Vehicle:<ns>-1` of the partial-update deletion cases, with two instances of each of a
/// `name`, a `locatedAt`, a `location` and a `street`. The first instance of each is in the dataset
/// `urn:ngsi-ld:Dataset:<ns>-01`. The second is in the dataset `urn:ngsi-ld:Dataset:<ns>-<second>`, or is the default
/// instance (no `datasetId`) when `second` is `None`.
pub(crate) fn building_two_instances(namespace: &Namespace, second: Option<&str>) -> Result<Entity<Input>> {
    let second_dataset = || second.map(|name| dataset(namespace, name)).transpose();
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Building")
        .property("name", Property::builder("Eiffel Tower").dataset_id(dataset(namespace, "01")?))
        .property("name", Property::builder("The iron lady").maybe_dataset_id(second_dataset()?))
        .relationship(
            "locatedAt",
            Relationship::builder(namespace.urn("City", "Paris")?).dataset_id(dataset(namespace, "01")?),
        )
        .relationship(
            "locatedAt",
            Relationship::builder(district_paris_08(namespace)?).maybe_dataset_id(second_dataset()?),
        )
        .geo_property("location", GeoProperty::builder(point(2.3514, 48.8575)?).dataset_id(dataset(namespace, "01")?))
        .geo_property("location", GeoProperty::builder(point(2.2944, 48.8585)?).maybe_dataset_id(second_dataset()?))
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([
                ("fr", "Jardin de la Tour Eiffel"),
                ("en", "Eiffel Tower Garden"),
            ])?)
            .dataset_id(dataset(namespace, "01")?),
        )
        .language_property(
            "street",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Avenue Gustave Eiffel"), ("en", "Avenue Gustave Eiffel")])?)
                .maybe_dataset_id(second_dataset()?),
        )
        .build()?)
}

/// The Building `urn:ngsi-ld:Building:<ns>-<suffix>` named in three datasets: Eiffel Tower by default, Tour Eiffel in
/// `urn:ngsi-ld:Dataset:<ns>-french-name` and Eiffelturm in `urn:ngsi-ld:Dataset:<ns>-german-name`; and located at
/// `urn:ngsi-ld:City:<ns>-Paris` by default, at `urn:ngsi-ld:Ville:<ns>-Paris` in the French dataset and at
/// `urn:ngsi-ld:Ciudad:<ns>-París` in `urn:ngsi-ld:Dataset:<ns>-spanish-name`.
pub(crate) fn building_named_in_datasets(namespace: &Namespace, suffix: impl Display) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", suffix)?)
        .entity_type("Building")
        .property("name", "Eiffel Tower")
        .property("name", Property::builder("Tour Eiffel").dataset_id(dataset(namespace, "french-name")?))
        .property("name", Property::builder("Eiffelturm").dataset_id(dataset(namespace, "german-name")?))
        .relationship("locatedAt", Relationship::builder(namespace.urn("City", "Paris")?))
        .relationship(
            "locatedAt",
            Relationship::builder(namespace.urn("Ville", "Paris")?).dataset_id(dataset(namespace, "french-name")?),
        )
        .relationship(
            "locatedAt",
            Relationship::builder(namespace.urn("Ciudad", "París")?).dataset_id(dataset(namespace, "spanish-name")?),
        )
        .build()?)
}

/// The Building `urn:ngsi-ld:Building:<ns>-<suffix>` whose `jsonProperty` holds an `id`, a `type`, a JSON `null`
/// member and an `anythingElse` member.
pub(crate) fn building_with_json_property(namespace: &Namespace, suffix: impl Display) -> Result<Entity<Input>> {
    let json: RawJson = r#"{"id":"some-id","type":"some-type","null":null,"anythingElse":"anything-else"}"#.parse()?;
    Ok(Entity::builder()
        .id(namespace.urn("Building", suffix)?)
        .entity_type("Building")
        .json_property("jsonProperty", JsonProperty::builder(json))
        .build()?)
}

/// The Building `urn:ngsi-ld:Building:<ns>-<suffix>` whose `vocabProperty` is `monument`.
pub(crate) fn building_with_vocab_property(namespace: &Namespace, suffix: impl Display) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Building", suffix)?)
        .entity_type("Building")
        .vocab_property("vocabProperty", VocabProperty::builder("monument".parse::<VocabTerm>()?))
        .build()?)
}

/// The City `urn:ngsi-ld:City:<ns>-<suffix>` named Paris, described as the capital of France, with an `isInCountry`
/// Relationship to the Country `urn:ngsi-ld:Country:<ns>-France`.
pub(crate) fn city_in_france_described(namespace: &Namespace, suffix: &str) -> Result<Entity<Input>> {
    city_paris_in_country(namespace, suffix, "France")
}

/// The polygon of `corners`, a closed ring of (longitude, latitude) positions.
pub(crate) fn polygon(corners: &[(f64, f64)]) -> Result<Shape> {
    let ring = LinearRing::try_new(
        corners
            .iter()
            .map(|&(longitude, latitude)| Ok(Position::try_new(longitude, latitude)?))
            .collect::<Result<Vec<_>>>()?,
    )?;
    Ok(Shape::Polygon(Polygon::try_new(vec![ring])?))
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the temporal cases: a `speed` of 120, 80 and 100 observed at 12:03,
/// 12:05 and 12:07, and a `fuelLevel` of 67, 53 and 40 observed at 12:03, 13:05 and 14:07, all on 2020-08-01 in UTC.
pub(crate) fn vehicle_speed_fuel_level(namespace: &Namespace) -> Result<TemporalEntity<Input>> {
    vehicle_speed_fuel_level_on(namespace, 1, "2020-08-01")
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-<suffix>` of the temporal query cases: a `speed` of 120, 80 and 100 observed
/// at 12:03, 12:05 and 12:07, and a `fuelLevel` of 67, 53 and 40 observed at 12:03, 13:05 and 14:07, all on `day`
/// (`YYYY-MM-DD`) in UTC.
pub(crate) fn vehicle_speed_fuel_level_on(namespace: &Namespace, suffix: impl Display, day: &str) -> Result<TemporalEntity<Input>> {
    speed_fuel_entity(
        namespace,
        "Vehicle",
        suffix,
        day,
        [(120, "12:03:00Z"), (80, "12:05:00Z"), (100, "12:07:00Z")],
        [(67, "12:03:00Z"), (53, "13:05:00Z"), (40, "14:07:00Z")],
    )
}

/// The Bus `urn:ngsi-ld:Bus:<ns>-<suffix>` of the temporal query cases: a `speed` of 45, 25 and 67 observed at 12:03,
/// 12:05 and 12:07, and a `fuelLevel` of 210, 145 and 124 observed at 12:03, 13:05 and 14:07, all on 2020-08-01 in
/// UTC.
pub(crate) fn bus_speed_fuel_level(namespace: &Namespace, suffix: impl Display) -> Result<TemporalEntity<Input>> {
    speed_fuel_entity(
        namespace,
        "Bus",
        suffix,
        "2020-08-01",
        [(45, "12:03:00Z"), (25, "12:05:00Z"), (67, "12:07:00Z")],
        [(210, "12:03:00Z"), (145, "13:05:00Z"), (124, "14:07:00Z")],
    )
}

/// The entity of type `kind` with id `urn:ngsi-ld:<kind>:<ns>-<suffix>`, whose `speed` and `fuelLevel` instances are
/// each `(value, time)` on `day`, the time written as `hh:mm:ssZ`.
fn speed_fuel_entity(
    namespace: &Namespace,
    kind: &str,
    suffix: impl Display,
    day: &str,
    speeds: [(i32, &str); 3],
    fuel_levels: [(i32, &str); 3],
) -> Result<TemporalEntity<Input>> {
    let mut builder = TemporalEntity::builder().id(namespace.urn(kind, suffix)?).entity_type(kind);
    for (value, time) in speeds {
        builder = builder.property("speed", Property::builder(value).observed_at(format!("{day}T{time}").parse()?));
    }
    for (value, time) in fuel_levels {
        builder = builder.property("fuelLevel", Property::builder(value).observed_at(format!("{day}T{time}").parse()?));
    }
    Ok(builder.build()?)
}

/// The instant `minute` past 01:00 on 2020-01-01 in UTC.
fn instant_at(minute: u32) -> Result<DateTime> {
    Ok(format!("2020-01-01T01:{minute:02}:00Z").parse()?)
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the cases that page through twenty instances: twenty speeds observed at
/// 01:01 to 01:20 and twenty fuel levels observed at 01:03 to 01:22, all on 2020-01-01 in UTC.
pub(crate) fn vehicle_twenty_instances(namespace: &Namespace) -> Result<TemporalEntity<Input>> {
    let mut builder = TemporalEntity::builder().id(namespace.urn("Vehicle", 1)?).entity_type("Vehicle");
    for minute in 1..=20 {
        builder = builder.property("speed", Property::builder(minute).observed_at(instant_at(minute)?));
    }
    for minute in 3..=22 {
        builder = builder.property("fuelLevel", Property::builder(minute - 2).observed_at(instant_at(minute)?));
    }
    Ok(builder.build()?)
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-<suffix>` of the query cases that page through five instances: a `speed` of
/// 85, 105, 120, 125 and 130 observed between 12:25 and 12:35, and a `fuelLevel` of 35, 31, 28, 24 and 19 observed
/// between 20:05 and 22:07, all on 2020-08-01 in UTC.
pub(crate) fn vehicle_five_instances(namespace: &Namespace, suffix: impl Display) -> Result<TemporalEntity<Input>> {
    let speeds = [
        (85, "2020-08-01T12:25:00Z"),
        (105, "2020-08-01T12:27:00Z"),
        (120, "2020-08-01T12:29:00Z"),
        (125, "2020-08-01T12:31:00Z"),
        (130, "2020-08-01T12:35:00Z"),
    ];
    let fuel_levels = [
        (35, "2020-08-01T20:05:00Z"),
        (31, "2020-08-01T20:07:00Z"),
        (28, "2020-08-01T21:03:00Z"),
        (24, "2020-08-01T21:05:00Z"),
        (19, "2020-08-01T22:07:00Z"),
    ];
    let mut builder = TemporalEntity::builder().id(namespace.urn("Vehicle", suffix)?).entity_type("Vehicle");
    for (value, observed_at) in speeds {
        builder = builder.property("speed", Property::builder(value).observed_at(observed_at.parse()?));
    }
    for (value, observed_at) in fuel_levels {
        builder = builder.property("fuelLevel", Property::builder(value).observed_at(observed_at.parse()?));
    }
    Ok(builder.build()?)
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the deleted-attribute cases: `isParkedIn` relationships to the
/// `OffStreetParking` Commerce and Graslin observed on 2024-12-01 and 2024-12-05, `name` language properties observed
/// on the same days, and `fuelLevel` properties of 67 and 53 observed on 2020-08-01 at 12:03 and 13:05, all in UTC.
pub(crate) fn vehicle_parking_name_fuel(namespace: &Namespace) -> Result<TemporalEntity<Input>> {
    Ok(TemporalEntity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .relationship(
            "isParkedIn",
            Relationship::builder(namespace.urn("OffStreetParking", "Commerce")?).observed_at("2024-12-01T12:00:00Z".parse()?),
        )
        .relationship(
            "isParkedIn",
            Relationship::builder(namespace.urn("OffStreetParking", "Graslin")?).observed_at("2024-12-05T12:00:00Z".parse()?),
        )
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Ma voiture"), ("en", "My vehicle")])?).observed_at("2024-12-01T12:00:00Z".parse()?),
        )
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("fr", "Ma jolie voiture"), ("en", "My pretty vehicle")])?)
                .observed_at("2024-12-05T12:00:00Z".parse()?),
        )
        .property("fuelLevel", Property::builder(67).observed_at("2020-08-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(53).observed_at("2020-08-01T13:05:00Z".parse()?))
        .build()?)
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the scope cases, without its `scope`, which the keyword that creates it
/// writes beside the rest: `fuelLevel` properties of 67 and 53 observed on 2020-08-01 at 12:03 and 13:05, in UTC.
pub(crate) fn vehicle_fuel_level(namespace: &Namespace) -> Result<TemporalEntity<Input>> {
    Ok(TemporalEntity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .property("fuelLevel", Property::builder(67).observed_at("2020-08-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(53).observed_at("2020-08-01T13:05:00Z".parse()?))
        .build()?)
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the temporal provision cases: a `speed` of 120 and 80 observed at 12:03
/// and 12:05, and a `fuelLevel` of 67, 53 and 40 observed at 12:03, 13:05 and 14:07, the last one in the dataset
/// `urn:ngsi-ld:Vehicle:<ns>-12345-fuel`, all on 2020-09-01 in UTC.
pub(crate) fn vehicle_speed_fuel_level_with_dataset(namespace: &Namespace) -> Result<TemporalEntity<Input>> {
    let fuel_dataset = DatasetId::from(namespace.urn("Vehicle", "12345-fuel")?.as_uri().clone());
    Ok(TemporalEntity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .property("speed", Property::builder(120).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("speed", Property::builder(80).observed_at("2020-09-01T12:05:00Z".parse()?))
        .property("fuelLevel", Property::builder(67).observed_at("2020-09-01T12:03:00Z".parse()?))
        .property("fuelLevel", Property::builder(53).observed_at("2020-09-01T13:05:00Z".parse()?))
        .property(
            "fuelLevel",
            Property::builder(40).observed_at("2020-09-01T14:07:00Z".parse()?).dataset_id(fuel_dataset),
        )
        .build()?)
}

/// The `datasetId` `urn:ngsi-ld:Property:<ns>-gpsBxyz123-speed` of the GPS `speed` instance of the replace cases.
pub(crate) fn gps_speed_dataset(namespace: &Namespace) -> Result<DatasetId> {
    Ok(DatasetId::from(namespace.urn("Property", "gpsBxyz123-speed")?.as_uri().clone()))
}

/// The `datasetId` `urn:ngsi-ld:Property:<ns>-speedometer` of the Speedometer `speed` instance.
pub(crate) fn speedometer_dataset(namespace: &Namespace) -> Result<DatasetId> {
    Ok(DatasetId::from(namespace.urn("Property", "speedometer")?.as_uri().clone()))
}

/// The `speed` of 56 in the dataset of [`speedometer_dataset`], with no source: the fragment a partial update writes
/// over that instance.
pub(crate) fn speedometer_speed_56_patch(namespace: &Namespace) -> Result<Attribute<Patch>> {
    Ok(Attribute::from(Property::builder(56).dataset_id(speedometer_dataset(namespace)?).build()?).into_patch())
}

/// The body of a partial update that changes only the `providedBy` Relationship to `urn:ngsi-ld:Person:<ns>-JohnDoe`.
/// It carries no payload of its own, which a typed attribute cannot hold.
pub(crate) fn provided_by_john_doe(namespace: &Namespace) -> Result<UncheckedDocument> {
    let provided_by = Attribute::from(Relationship::builder(namespace.urn("Person", "JohnDoe")?).build()?).render(Lossless::Normalized);
    Ok(UncheckedDocument::empty().set_member("providedBy", &provided_by))
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the replace cases: scope `/Madrid/Centro`, the `isParked` of
/// [`is_parked_downtown1`], and two `speed` instances, 55 from a Speedometer and 54.5 from a GPS in the dataset of
/// [`gps_speed_dataset`].
pub(crate) fn vehicle_parked_speed_sources(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .scope("/Madrid/Centro")
        .relationship("isParked", is_parked_downtown1(namespace)?)
        .property("speed", Property::builder(55).property("source", Property::builder("Speedometer")))
        .property(
            "speed",
            Property::builder(Value::try_from(54.5)?)
                .property("source", Property::builder("GPS"))
                .dataset_id(gps_speed_dataset(namespace)?),
        )
        .build()?)
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the delete cases: a `brandName` of Mercedes, an `isParked` Relationship
/// to the `OffStreetParking` Downtown1 observed on 2017-07-29 at 12:00:04 in UTC, provided by Bob, in the dataset
/// `urn:ngsi-ld:Relationship:<ns>-parked12345`, and two `speed` instances: 55 from a Speedometer as the default
/// instance, and 54.5 from a GPS in the dataset of [`gps_speed_dataset`].
pub(crate) fn vehicle_brand_parked_speed_gps(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .property("brandName", "Mercedes")
        .relationship(
            "isParked",
            Relationship::builder(namespace.urn("OffStreetParking", "Downtown1")?)
                .observed_at("2017-07-29T12:00:04Z".parse()?)
                .relationship("providedBy", namespace.urn("Person", "Bob")?)
                .dataset_id(parked_dataset(namespace)?),
        )
        .property("speed", Property::builder(55).property("source", Property::builder("Speedometer")))
        .property(
            "speed",
            Property::builder(Value::try_from(54.5)?)
                .property("source", Property::builder("GPS"))
                .dataset_id(gps_speed_dataset(namespace)?),
        )
        .build()?)
}

/// The `speed` instance the partial-update cases send: 129 observed at 2020-09-01T12:03:00Z in UTC.
pub(crate) fn replacement_speed_instance() -> Result<Attribute<Input>> {
    Ok(Property::builder(129).observed_at("2020-09-01T12:03:00Z".parse()?).build()?.into())
}

/// The instances of the attribute `added_attribute` that the add-attributes cases send: 120, 80 and 100 observed at
/// 12:03, 12:05 and 12:07 on 2020-09-01 in UTC.
pub(crate) fn added_attribute_instances() -> Result<TemporalAttributes<Input>> {
    let mut attributes = TemporalAttributes::<Input>::new();
    for (value, observed_at) in [(120, "2020-09-01T12:03:00Z"), (80, "2020-09-01T12:05:00Z"), (100, "2020-09-01T12:07:00Z")] {
        let name: AttributeName = "added_attribute".parse()?;
        attributes.insert(name, Property::builder(value).observed_at(observed_at.parse()?).build()?.into())?;
    }
    Ok(attributes)
}

/// The `datasetId` `urn:ngsi-ld:Dataset:<ns>-<name>`.
pub(crate) fn dataset(namespace: &Namespace, name: &str) -> Result<DatasetId> {
    Ok(DatasetId::from(namespace.urn("Dataset", name)?.as_uri().clone()))
}

/// The `datasetId` `urn:ngsi-ld:Relationship:<ns>-parked12345` of the `isParked` Relationship of the Vehicles that the
/// cases load.
pub(crate) fn parked_dataset(namespace: &Namespace) -> Result<DatasetId> {
    Ok(DatasetId::from(namespace.urn("Relationship", "parked12345")?.as_uri().clone()))
}

/// The District `urn:ngsi-ld:District:Paris:<ns>-08`, whose namespace token follows the `Paris:` segment.
pub(crate) fn district_paris_08(namespace: &Namespace) -> Result<EntityId> {
    Ok(format!("urn:ngsi-ld:District:Paris:{namespace}-08").parse()?)
}

/// The terms of the `@context` the append cases on the Vehicle of [`vehicle_brand_parked_speed_sources`] register.
pub(crate) const VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS: [&str; 6] = ["Vehicle", "brandName", "isParked", "providedBy", "speed", "source"];

/// The terms of the `@context` the append cases that send `attribute_to_be_added` register: the terms of
/// [`VEHICLE_BRAND_PARKED_SPEED_SOURCE_TERMS`] and the appended `attribute_to_be_added`.
pub(crate) const VEHICLE_BRAND_PARKED_SPEED_SOURCE_ADDED_TERMS: [&str; 7] =
    ["Vehicle", "brandName", "isParked", "providedBy", "speed", "source", "attribute_to_be_added"];

/// The Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` of the append cases: a `brandName` of Mercedes, the `isParked` of
/// [`is_parked_downtown1`], and two `speed` instances, 55 from a Speedometer in the dataset
/// `urn:ngsi-ld:Property:<ns>-speedometer` and 54.5 from a GPS in the dataset of [`gps_speed_dataset`].
pub(crate) fn vehicle_brand_parked_speed_sources(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Vehicle")
        .property("brandName", "Mercedes")
        .relationship("isParked", is_parked_downtown1(namespace)?)
        .property(
            "speed",
            Property::builder(55)
                .property("source", Property::builder("Speedometer"))
                .dataset_id(speedometer_dataset(namespace)?),
        )
        .property(
            "speed",
            Property::builder(Value::try_from(54.5)?)
                .property("source", Property::builder("GPS"))
                .dataset_id(gps_speed_dataset(namespace)?),
        )
        .build()?)
}

/// The Speedometer `speed` of 56 in the dataset `urn:ngsi-ld:Property:<ns>-<dataset>`, which the append cases add.
pub(crate) fn speedometer_speed_56(namespace: &Namespace, dataset: &str) -> Result<Property<Input>> {
    Ok(Property::builder(56)
        .property("source", Property::builder("Speedometer"))
        .dataset_id(DatasetId::from(namespace.urn("Property", dataset)?.as_uri().clone()))
        .build()?)
}

/// The fragment the append cases send beside `attribute_to_be_added`: the Speedometer `speed` of 56 of
/// [`speedometer_speed_56`] in the dataset `dataset`, and an `attribute_to_be_added` of 23.
pub(crate) fn speed_and_added_attribute(namespace: &Namespace, dataset: &str) -> Result<EntityFragment> {
    Ok(EntityFragment::builder()
        .property("speed", speedometer_speed_56(namespace, dataset)?)
        .property("attribute_to_be_added", 23)
        .build()?)
}

/// The fragment that the update cases send: the Speedometer `speed` of 99 without a dataset, which replaces the
/// default instance of the setup.
pub(crate) fn speedometer_speed_99_fragment() -> Result<EntityFragment> {
    Ok(EntityFragment::builder()
        .property("speed", Property::builder(99).property("source", Property::builder("Speedometer")))
        .build()?)
}

/// The fragment that appends the scope `/CompanyA/OrganizationB/UnitC` to an entity.
pub(crate) fn company_unit_scope() -> Result<EntityFragment> {
    let scope: Scope = "/CompanyA/OrganizationB/UnitC".parse()?;
    Ok(EntityFragment::builder().scope(ScopeChange::Set(OneOrMany::from(scope))).build()?)
}

/// The Building `urn:ngsi-ld:Vehicle:<ns>-1` of the update cases that name a type or a scope, with no scope of its
/// own.
pub(crate) fn vehicle_id_building(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder().id(namespace.urn("Vehicle", 1)?).entity_type("Building").build()?)
}

/// The Building `urn:ngsi-ld:Vehicle:<ns>-1` whose `scope` is `/Madrid/Gardens/ParqueNorte`, for the update cases that
/// replace a scope.
pub(crate) fn vehicle_id_building_with_scope(namespace: &Namespace) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type("Building")
        .scope("/Madrid/Gardens/ParqueNorte")
        .build()?)
}
