//! The Building entities the conformance tests load, namespaced for the test that loads them, and the terms of the
//! `@context` each family of Building tests registers.

use crate::{geometry_payloads::point, linked_place_payloads::district_paris_08, vehicle_entity_payloads::dataset};
use andromeda_test_suite::namespace::Namespace;
use cefact_units::UnitCode;
use googletest::Result;
use ngsi_ld::model::{
    AttributeDraft,
    Entity,
    EntityFragment,
    EntityId,
    GeoProperty,
    Input,
    JsonProperty,
    LanguageMap,
    LanguageProperty,
    OneOrMany,
    Property,
    RawJson,
    Relationship,
    Scope,
    ScopeChange,
    VocabProperty,
    VocabTerm,
};
use std::fmt::Display;

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

/// The terms of the `@context` the Merge Entity cases on a Building that holds a name, a `locatedAt` and a `street`.
pub(crate) const BUILDING_LOCATED_STREET_TERMS: [&str; 4] = ["Building", "name", "locatedAt", "street"];

/// The terms of the `@context` the partial-update cases on a Building with a `jsonProperty` register.
pub(crate) const BUILDING_JSON_PROPERTY_TERMS: [&str; 4] = ["Building", "jsonProperty", "source", "providedBy"];

/// The terms of the `@context` the partial-update cases on a Building with a `vocabProperty` register.
pub(crate) const BUILDING_VOCAB_PROPERTY_TERMS: [&str; 4] = ["Building", "vocabProperty", "source", "providedBy"];

/// The terms of the `@context` the sub-attribute removal cases on a Building with an `airQualityLevel` that holds a
/// `hasWeatherObserved` Relationship register.
pub(crate) const BUILDING_AIR_QUALITY_WEATHER_TERMS: [&str; 6] = ["Building", "name", "subCategory", "airQualityLevel", "hasWeatherObserved", "almostFull"];

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
        .property("almostFull", false)
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
        .property("almostFull", true)
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
        .property("almostFull", false)
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

/// The Building `urn:ngsi-ld:Building:<ns>-<suffix>` with no attribute.
pub(crate) fn bare_building(namespace: &Namespace, suffix: impl Display) -> Result<Entity<Input>> {
    Ok(Entity::builder().id(namespace.urn("Building", suffix)?).entity_type("Building").build()?)
}

/// The entity `urn:ngsi-ld:Building:<ns>-<suffix>` with no attribute, of the types `types`.
pub(crate) fn building_of_types<const N: usize>(namespace: &Namespace, suffix: impl Display, types: [&str; N]) -> Result<Entity<Input>> {
    Ok(Entity::builder().id(namespace.urn("Building", suffix)?).entity_type(types).build()?)
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
