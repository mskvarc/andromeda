//! The Vehicle the temporal version-adaptation tests share: one temporal evolution holding every attribute family that
//! an older version amends (a second type, a scope, three speed instances with a unit and a dataset, a
//! `LanguageProperty`, a Relationship with its `objectType`), so a read under a `Prefer: ngsi-ld=<version>` shows how
//! each is amended (clause 4.3.6.8, Tables 4.3.6.8-1 to 4.3.6.8-3).

use crate::namespace::{Namespace, NamespaceError};
use cefact_units::UnitCode;
use ngsi_ld::model::{
    BuildError,
    DateTime,
    Input,
    LanguageMap,
    LanguageMapError,
    LanguageProperty,
    NameError,
    Property,
    Relationship,
    Scope,
    ScopeError,
    TemporalError,
    VocabTerm,
    evolution::{ScopeInstance, TemporalEntity},
};

/// Why the Vehicle could not be built.
#[derive(Debug, thiserror::Error)]
pub enum TemporalPreferVehicleError {
    /// A name or identifier of the test is invalid.
    #[error(transparent)]
    Namespace(#[from] NamespaceError),
    /// A name of the Vehicle is invalid.
    #[error(transparent)]
    Name(#[from] NameError),
    /// An instant of the Vehicle is invalid.
    #[error(transparent)]
    Instant(#[from] TemporalError),
    /// The language map of the Vehicle is invalid.
    #[error(transparent)]
    Language(#[from] LanguageMapError),
    /// The scope of the Vehicle is invalid.
    #[error("the scope is invalid")]
    Scope(#[source] ScopeError),
    /// A member of the evolution is invalid.
    #[error(transparent)]
    Build(#[from] BuildError),
}

/// The evolution `urn:ngsi-ld:Vehicle:<token>-<suffix>` of the types `Vehicle` and `Car`, with the scope
/// `/regression/temporalPrefer`, three `speed` instances (1, 2 and 3 on the days 1 to 3 of January 2026, in `KMH`,
/// of the dataset `primary`), a `label` `LanguageProperty` (`en`: Pump) and a `refBuilding` Relationship to the
/// Building `target` of the object type `Building`.
///
/// # Errors
///
/// Returns [`TemporalPreferVehicleError`] when a name, an instant or a member is invalid.
pub fn vehicle(namespace: &Namespace, suffix: &str) -> Result<TemporalEntity<Input>, TemporalPreferVehicleError> {
    let primary = namespace.dataset_id("Dataset", "primary")?;
    let speed = |value: i32, day: &str| -> Result<Property<Input>, TemporalPreferVehicleError> {
        Ok(Property::builder(value)
            .observed_at(day.parse::<DateTime>()?)
            .unit_code(UnitCode::Kmh)
            .dataset_id(primary.clone())
            .build()?)
    };
    let scope = "/regression/temporalPrefer".parse::<Scope>().map_err(TemporalPreferVehicleError::Scope)?;
    Ok(TemporalEntity::builder()
        .id(namespace.urn("Vehicle", suffix)?)
        .entity_type(vec![namespace.entity_type("Vehicle")?, namespace.entity_type("Car")?])
        .scope(ScopeInstance::new(scope))
        .property("speed", speed(1, "2026-01-01T00:00:00Z")?)
        .property("speed", speed(2, "2026-01-02T00:00:00Z")?)
        .property("speed", speed(3, "2026-01-03T00:00:00Z")?)
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Pump")])?).observed_at("2026-01-01T00:00:00Z".parse::<DateTime>()?),
        )
        .relationship(
            "refBuilding",
            Relationship::builder(namespace.urn("Building", "target")?)
                .object_type("Building".parse::<VocabTerm>()?)
                .observed_at("2026-01-01T00:00:00Z".parse::<DateTime>()?),
        )
        .build()?)
}
