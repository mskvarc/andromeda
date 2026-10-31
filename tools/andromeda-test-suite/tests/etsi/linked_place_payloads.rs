//! The Buildings, Cities, Countries and Districts the linked-entity cases chain by `locatedAt` and `isInCountry`,
//! and the terms of the `@context` those cases register.

use andromeda_test_suite::namespace::Namespace;
use googletest::Result;
use ngsi_ld::model::{Entity, EntityId, Input, Relationship, VocabTerm};

/// The terms of the `@context` the linked-entity cases register for a Building located at a City in a Country.
pub(crate) const PARIS_IN_FRANCE_TERMS: [&str; 6] = ["Building", "locatedAt", "City", "name", "isInCountry", "Country"];

/// The terms of the `@context` the linked cases with a street and a location register, as `create_street_linked_pairs`
/// names them.
pub(crate) const STREET_LINKED_TERMS: [&str; 6] = ["Building", "name", "locatedAt", "street", "City", "isInCountry"];

/// The Building of the linked-entity cases, `urn:ngsi-ld:Building:<ns>-<suffix>`, with a `locatedAt` Relationship to
/// [`city_paris`].
pub(crate) fn building_located_at(namespace: &Namespace, suffix: &str) -> Result<Entity<Input>> {
    let located_at = Relationship::builder(namespace.urn("City", "Paris")?).object_type("City".parse::<VocabTerm>()?);
    Ok(Entity::builder()
        .id(namespace.urn("Building", suffix)?)
        .entity_type("Building")
        .relationship("locatedAt", located_at)
        .build()?)
}

/// The Building `urn:ngsi-ld:Building:<ns>-<suffix>` with a `locatedAt` Relationship to the City
/// `urn:ngsi-ld:City:<ns>-<suffix>`, whose `objectType` is `City`.
pub(crate) fn building_located_at_city(namespace: &Namespace, suffix: &str) -> Result<Entity<Input>> {
    let located_at = Relationship::builder(namespace.urn("City", suffix)?).object_type("City".parse::<VocabTerm>()?);
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
    let located_at = Relationship::builder(namespace.urn("Country", suffix)?).object_type("Country".parse::<VocabTerm>()?);
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

/// The City `urn:ngsi-ld:City:<ns>-<suffix>` named Paris, described as the capital of France, with an `isInCountry`
/// Relationship to the Country `urn:ngsi-ld:Country:<ns>-France`.
pub(crate) fn city_in_france_described(namespace: &Namespace, suffix: &str) -> Result<Entity<Input>> {
    city_paris_in_country(namespace, suffix, "France")
}

/// The District `urn:ngsi-ld:District:Paris:<ns>-08`, whose namespace token follows the `Paris:` segment.
pub(crate) fn district_paris_08(namespace: &Namespace) -> Result<EntityId> {
    Ok(format!("urn:ngsi-ld:District:Paris:{namespace}-08").parse()?)
}
