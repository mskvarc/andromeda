//! The Vehicle of Annex C.2.1 and C.2.2, the Entities its Relationships target, and the aggregated `@context`
//! (common terms, vehicle, parking, then the core `@context`) every request about them names.

use crate::served_context::{ServedContexts, core_context};
use andromeda_test_suite::{broker::BrokerClient, namespace::Namespace, problem::answered};
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{DateTime, Entity, EntityId, Input, ListProperty, ListRelationship, Relationship, Value, VocabTerm};
use serde_json::json;
use url::Url;

/// Serves the example's common terms, vehicle and parking documents and the aggregated `@context` listing them before
/// the core `@context`, and answers the aggregated one's URL.
pub(crate) async fn aggregated_context(served: &ServedContexts, namespace: &Namespace) -> Result<Url> {
    let common = json!({"@context": {
        "isParked": {"@type": "@id", "@id": "http://example.org/common/isParked"},
        "isNextToBuilding": {"@type": "@id", "@id": "http://example.org/common/isNextToBuilding"},
        "operatedBy": {"@type": "@id", "@id": "http://example.org/common/operatedBy"},
        "providedBy": {"@type": "@id", "@id": "http://example.org/common/providedBy"},
        "reliability": "http://example.org/common/reliability",
        "name": "http://example.org/common/name",
        namespace.suffixed("Person"): format!("http://example.org/common/{}", namespace.suffixed("Person")),
        namespace.suffixed("City"): format!("http://example.org/common/{}", namespace.suffixed("City")),
        namespace.suffixed("Camera"): format!("http://example.org/common/{}", namespace.suffixed("Camera")),
        namespace.suffixed("Company"): format!("http://example.org/common/{}", namespace.suffixed("Company")),
    }});
    let vehicle = json!({"@context": {
        namespace.suffixed("Vehicle"): format!("http://example.org/vehicle/{}", namespace.suffixed("Vehicle")),
        "street": "http://example.org/vehicle/street",
        "brandName": "http://example.org/vehicle/brandName",
        "category": "http://example.org/vehicle/category",
        "tyreTreadDepths": "http://example.org/vehicle/tyreTreadDepths",
        "passengers": "http://example.org/vehicle/passengers",
        "route": "http://example.org/vehicle/route",
        "marque": "http://example.org/vehicle/marque",
        "speed": "http://example.org/vehicle/speed",
        "commercial": "http://example.org/vehicle/commercial",
        "non-commercial": "http://example.org/vehicle/non-commercial",
        "Integer": "http://www.w3.org/2001/XMLSchema#Integer",
    }});
    let parking = json!({"@context": {
        namespace.suffixed("OffStreetParking"): format!("http://example.org/parking/{}", namespace.suffixed("OffStreetParking")),
        "availableSpotNumber": "http://example.org/parking/availableSpotNumber",
        "totalSpotNumber": "http://example.org/parking/totalSpotNumber",
    }});
    let common = served.serve("commonTerms.jsonld", &common).await?;
    let vehicle = served.serve("vehicle.jsonld", &vehicle).await?;
    let parking = served.serve("parking.jsonld", &parking).await?;
    let aggregated = json!({"@context": [common.as_str(), vehicle.as_str(), parking.as_str(), core_context()?.as_str()]});
    served.serve("aggregatedContext.jsonld", &aggregated).await
}

/// The named Entity `<kind>:<ns>-<suffix>` of type `<kind>_<ns>` whose `name` is `name`.
fn named(namespace: &Namespace, kind: &str, suffix: &str, name: &str) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn(kind, suffix)?)
        .entity_type(namespace.entity_type(kind)?)
        .property("name", name)
        .build()?)
}

/// The Entities the Vehicle's Relationships target: the parking Downtown1 operated by `BigParkingCorp`, the persons
/// Alice and Bob, and the cities Antwerp, Rotterdam and Amsterdam.
pub(crate) fn linked_entities(namespace: &Namespace) -> Result<Vec<Entity<Input>>> {
    let parking = Entity::builder()
        .id(namespace.urn("OffStreetParking", "Downtown1")?)
        .entity_type(namespace.entity_type("OffStreetParking")?)
        .property("name", "Top Parking")
        .relationship("operatedBy", namespace.urn("Company", "BigParkingCorp")?)
        .build()?;
    Ok(vec![
        parking,
        named(namespace, "Person", "Alice", "Alice")?,
        named(namespace, "Person", "Bob", "Bob")?,
        named(namespace, "City", "Antwerp", "Antwerp")?,
        named(namespace, "City", "Rotterdam", "Rotterdam")?,
        named(namespace, "City", "Amsterdam", "Amsterdam")?,
    ])
}

/// The Vehicle A4567 of the example, with every attribute kind it carries, its `tyreTreadDepths` in `tyre_unit` when
/// one is given.
pub(crate) fn vehicle(namespace: &Namespace, tyre_unit: Option<UnitCode>) -> Result<Entity<Input>> {
    let is_parked = Relationship::builder(namespace.urn("OffStreetParking", "Downtown1")?)
        .object_type(namespace.entity_type("OffStreetParking")?)
        .observed_at("2017-07-29T12:00:04Z".parse::<DateTime>()?)
        .relationship("providedBy", namespace.urn("Person", "Bob")?);
    let tyre_tread_depths = ListProperty::builder(vec![Value::from(300), Value::from(300), Value::from(120), Value::from(6)])
        .value_type("Integer".parse::<VocabTerm>()?)
        .maybe_unit_code(tyre_unit);
    let passengers =
        Relationship::builder(vec![namespace.urn("Person", "Alice")?, namespace.urn("Person", "Bob")?]).object_type(namespace.entity_type("Person")?);
    let route: Vec<EntityId> = vec![
        namespace.urn("City", "Antwerp")?,
        namespace.urn("City", "Rotterdam")?,
        namespace.urn("City", "Amsterdam")?,
    ];
    Ok(Entity::builder()
        .id(namespace.urn("Vehicle", "A4567")?)
        .entity_type(namespace.entity_type("Vehicle")?)
        .property("brandName", "Mercedes")
        .language_property("street", [("fr", "Grand Place"), ("nl", "Grote Markt")])
        .relationship("isParked", is_parked)
        .vocab_property("category", "non-commercial")
        .list_property("tyreTreadDepths", tyre_tread_depths)
        .relationship("passengers", passengers)
        .list_relationship("route", ListRelationship::builder(route).object_type(namespace.entity_type("City")?))
        .build()?)
}

/// Creates the linked Entities in one batch, requiring 201, then the Vehicle, through `client`.
pub(crate) async fn create_vehicle_example(client: &BrokerClient, namespace: &Namespace) -> Result<Entity<Input>> {
    let created = client.with_responses().entities().batch_create(&linked_entities(namespace)?).send().await;
    verify_that!(&created, answered(StatusCode::CREATED))?;
    let vehicle = vehicle(namespace, None)?;
    client.entities().create(&vehicle).send().await?;
    Ok(vehicle)
}
