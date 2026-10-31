//! The Vehicle the conformance tests notify: one entity holding every datatype and member that a conformance version
//! older than the broker's replaces by a fallback (clause 4.3.6.8), so a notification of it shows which fallbacks a
//! subscription asked for.

use crate::namespace::{Namespace, NamespaceError};
use cefact_units::UnitCode;
use ngsi_ld::model::{
    BuildError,
    Entity,
    Input,
    InvalidJson,
    JsonProperty,
    LanguageMap,
    LanguageMapError,
    LanguageProperty,
    ListProperty,
    ListRelationship,
    NameError,
    Property,
    RawJson,
    Relationship,
    TemporalError,
    Value,
    VocabProperty,
    VocabTerm,
};

/// Why the Vehicle could not be built.
#[derive(Debug, thiserror::Error)]
pub enum ConformanceVehicleError {
    /// The entity is not valid.
    #[error("the entity could not be built")]
    Entity(#[from] BuildError),
    /// An identifier or type is not valid.
    #[error("an identifier is not valid")]
    Namespace(#[from] NamespaceError),
    /// A name is not valid.
    #[error("a name is not valid")]
    Name(#[from] NameError),
    /// An instant is not valid.
    #[error("an instant is not valid")]
    Instant(#[from] TemporalError),
    /// A JSON value is not valid.
    #[error("a JSON value is not valid")]
    Json(#[from] InvalidJson),
    /// A language map is not valid.
    #[error("a language map is not valid")]
    Language(#[from] LanguageMapError),
}

/// The Vehicle `urn:ngsi-ld:Vehicle:<token>-<suffix>` of the types `Vehicle_<token>` and `Car_<token>`, with the scope
/// `/conformance`, an `expiresAt`, a Property with `observedAt`, `unitCode` and `valueType`, a `LanguageProperty`, a
/// `VocabProperty`, a `JsonProperty`, a `ListProperty`, a `ListRelationship` of two rooms, a Relationship with
/// `objectType` and `observedAt`, and a Property of two instances.
///
/// # Errors
///
/// Returns [`ConformanceVehicleError`] when a member is not valid.
pub fn vehicle(namespace: &Namespace, suffix: &str) -> Result<Entity<Input>, ConformanceVehicleError> {
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", suffix)?)
        .entity_type(vec![namespace.entity_type("Vehicle")?, namespace.entity_type("Car")?])
        .scope("/conformance")
        .expires_at("2099-01-01T00:00:00Z".parse()?)
        .property(
            "speed",
            Property::builder(20)
                .observed_at("2025-01-01T00:00:00Z".parse()?)
                .unit_code(UnitCode::Cel)
                .value_type("Number".parse::<VocabTerm>()?),
        )
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Tower"), ("de", "Turm")])?),
        )
        .vocab_property("category", VocabProperty::builder("commercial".parse::<VocabTerm>()?))
        .json_property("config", JsonProperty::builder(r#"{"level": 3}"#.parse::<RawJson>()?))
        .list_property("tags", ListProperty::builder(vec![Value::from("a"), Value::from("b")]))
        .list_relationship(
            "refRooms",
            ListRelationship::builder(vec![namespace.urn("Room", suffix)?, namespace.urn("Room", format!("{suffix}-second"))?]),
        )
        .relationship(
            "refBuilding",
            Relationship::builder(namespace.urn("Building", suffix)?)
                .object_type("Building".parse::<VocabTerm>()?)
                .observed_at("2025-01-01T00:00:00Z".parse()?),
        )
        .property("temperature", Property::builder(25).dataset_id(namespace.dataset_id("Dataset", "one")?))
        .property("temperature", 30)
        .build()?)
}

#[cfg(test)]
mod tests {
    use crate::{conformance_vehicle::vehicle, namespace::Namespace};
    use googletest::prelude::*;

    #[gtest]
    fn the_vehicle_is_named_after_the_namespace_and_suffix() -> Result<()> {
        let namespace = Namespace::fresh();
        let built = vehicle(&namespace, "probe")?;
        verify_that!(built.id, eq(&namespace.urn("Vehicle", "probe")?))
    }
}
