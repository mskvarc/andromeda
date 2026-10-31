//! The Vehicle the version-adaptation tests share: one entity holding every attribute family, so a read under a
//! `Prefer: ngsi-ld=<version>` shows how each family is amended for an older version (clause 4.3.6.8, Table
//! 4.3.6.8-1).

use crate::{
    broker::BrokerClient,
    namespace::{Namespace, NamespaceError},
};
use cefact_units::UnitCode;
use http::StatusCode;
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::{UncheckedDocument, operation::CreateEntity},
    model::{
        BuildError,
        DateTime,
        Entity,
        EntityId,
        GeoProperty,
        Input,
        InvalidJson,
        JsonProperty,
        LanguageMap,
        LanguageMapError,
        LanguageProperty,
        ListProperty,
        Lossless,
        NameError,
        Property,
        RawJson,
        Relationship,
        TemporalError,
        Value,
        VocabProperty,
        VocabTerm,
        geometry::{GeometryError, Position, Shape},
    },
};

/// Whether the `label` attribute of the Vehicle holds one instance or two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelInstances {
    /// One `LanguageProperty` instance with the languages `en` (Store) and `de` (Laden).
    One,
    /// A `LanguageProperty` instance for the dataset `secondary` (`en`: Store) and a default one (`en`: Depot).
    Datasets,
}

/// Why the Vehicle could not be built or created.
#[derive(Debug, thiserror::Error)]
pub enum VersionVehicleError {
    /// A name or identifier of the test is invalid.
    #[error(transparent)]
    Namespace(#[from] NamespaceError),
    /// An instant of the Vehicle is invalid.
    #[error(transparent)]
    Instant(#[from] TemporalError),
    /// A term of the Vehicle is invalid.
    #[error(transparent)]
    Name(#[from] NameError),
    /// A language map of the Vehicle is invalid.
    #[error(transparent)]
    Language(#[from] LanguageMapError),
    /// The position of the Vehicle is invalid.
    #[error(transparent)]
    Geometry(#[from] GeometryError),
    /// A JSON member of the Vehicle is invalid.
    #[error(transparent)]
    Json(#[from] InvalidJson),
    /// A member of the Vehicle is invalid.
    #[error(transparent)]
    Build(#[from] BuildError),
    /// The broker refused the create or the transport failed.
    #[error("the vehicle could not be created")]
    Create(#[source] Box<ClientError<ReqwestError>>),
    /// The broker answered a success other than 201.
    #[error("the broker answered {0} to the create")]
    Status(StatusCode),
}

/// The entity `urn:ngsi-ld:Vehicle:<token>-<suffix>` of the types `Vehicle` and `Car`, with a scope, an `expiresAt`
/// and a Property with its `unitCode`, `valueType` and `observedAt`, a `GeoProperty`, a Relationship with its
/// `objectType`, a `LanguageProperty`, a `VocabProperty`, a `JsonProperty`, a `ListProperty` and a Property of two dataset
/// instances; the `ListRelationship` `refRooms` is added to the document by [`create`].
///
/// # Errors
///
/// Returns [`VersionVehicleError`] when a name or a member is invalid.
pub fn vehicle(namespace: &Namespace, suffix: &str, label: LabelInstances) -> Result<Entity<Input>, VersionVehicleError> {
    let observed_at = "2025-01-01T00:00:00Z".parse::<DateTime>()?;
    let expires_at = "2099-01-01T00:00:00Z".parse::<DateTime>()?;
    let target_id = namespace.urn("Building", "target")?;
    let secondary = namespace.dataset_id("Dataset", "secondary")?;
    let both = LanguageMap::try_from_pairs([("en", "Store"), ("de", "Laden")])?;
    let store = LanguageMap::try_from_pairs([("en", "Store")])?;
    let depot = LanguageMap::try_from_pairs([("en", "Depot")])?;
    let builder = Entity::builder()
        .id(namespace.urn("Vehicle", suffix)?)
        .entity_type(vec![namespace.entity_type("Vehicle")?, namespace.entity_type("Car")?])
        .scope(["/conformance"])
        .expires_at(expires_at)
        .property(
            "speed",
            Property::builder(21)
                .observed_at(observed_at)
                .unit_code(UnitCode::Cel)
                .value_type("Number".parse::<VocabTerm>()?),
        )
        .geo_property("spot", GeoProperty::builder(Shape::Point(Position::try_new(1.5, 2.5)?)))
        .relationship(
            "refBuilding",
            Relationship::builder(target_id)
                .object_type("Building".parse::<VocabTerm>()?)
                .observed_at(observed_at),
        );
    let builder = match label {
        LabelInstances::One => builder.language_property("label", LanguageProperty::builder(both)),
        LabelInstances::Datasets => builder
            .language_property("label", LanguageProperty::builder(store).dataset_id(secondary.clone()))
            .language_property("label", LanguageProperty::builder(depot)),
    };
    Ok(builder
        .vocab_property("category", VocabProperty::builder("Building".parse::<VocabTerm>()?))
        .json_property("config", JsonProperty::builder(r#"{"a": 1}"#.parse::<RawJson>()?))
        .list_property("tags", ListProperty::builder(vec![Value::from("a"), Value::from("b")]))
        .property("pressure", Property::builder(25).dataset_id(secondary))
        .property("pressure", 30)
        .build()?)
}

/// The `ListRelationship` `refRooms` of the rooms `first` and `second` of `namespace`, its `objectList` the plain list of
/// identifiers clause 5.2.37 gives, which the model does not write.
///
/// # Errors
///
/// Returns [`InvalidJson`] when the member is not JSON.
fn ref_rooms(namespace: &Namespace) -> Result<RawJson, InvalidJson> {
    format!(r#"{{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:{namespace}-first", "urn:ngsi-ld:Room:{namespace}-second"]}}"#).parse()
}

/// Creates the Vehicle of [`vehicle`] through `client` with the `ListRelationship` `refRooms` of the rooms `first` and
/// `second`, whose `objectList` is written as the plain list of identifiers clause 5.2.37 gives, and returns its id.
///
/// # Errors
///
/// Returns [`VersionVehicleError`] when the Vehicle is invalid, the create fails or the broker does not answer 201.
pub async fn create(client: &BrokerClient, namespace: &Namespace, suffix: &str, label: LabelInstances) -> Result<EntityId, VersionVehicleError> {
    let vehicle = vehicle(namespace, suffix, label)?;
    let ref_rooms = ref_rooms(namespace)?;
    let document = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized).set_member("refRooms", &ref_rooms);
    let request = client
        .unchecked(&CreateEntity::builder().entity(&vehicle).build())
        .map_err(|error| VersionVehicleError::Create(Box::new(error)))?
        .document(&document);
    let response = client
        .send_unchecked(request)
        .await
        .map_err(|error| VersionVehicleError::Create(Box::new(error)))?;
    if response.status() == StatusCode::CREATED {
        Ok(vehicle.id)
    } else {
        Err(VersionVehicleError::Status(response.status()))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        namespace::Namespace,
        version_vehicle::{LabelInstances, vehicle},
    };
    use googletest::prelude::*;

    #[gtest]
    fn the_vehicle_holds_eight_attributes_beside_the_list_relationship() -> Result<()> {
        let namespace = Namespace::fresh();
        let entity = vehicle(&namespace, "families", LabelInstances::One)?;
        expect_that!(entity.attributes.len(), eq(8));
        expect_that!(entity.scope.is_some(), eq(true));
        verify_that!(entity.expires_at.is_some(), eq(true))
    }

    #[gtest]
    fn a_label_of_datasets_holds_two_instances() -> Result<()> {
        let namespace = Namespace::fresh();
        let entity = vehicle(&namespace, "datasets", LabelInstances::Datasets)?;
        let label = entity
            .attributes
            .iter()
            .find(|(name, _)| name.as_str() == "label")
            .map(|(_, instances)| instances.len());
        verify_that!(label, some(eq(2)))
    }
}
