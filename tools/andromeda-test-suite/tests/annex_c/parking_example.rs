//! The `OffStreetParking` Entities of Annex C.2.3 and C.5.15, and how the annex prints them in each representation,
//! as Entities and as `GeoJSON` Features.

use andromeda_test_suite::{broker::BrokerClient, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{
    DateTime,
    Entity,
    Input,
    OneOrMany,
    Property,
    Representation,
    Scope,
    Value,
    geometry::{Position, Shape},
};
use serde_json::{Map, Value as Json, json};

/// What the annex states of one parking.
pub(crate) struct Parking {
    /// The id suffix.
    pub(crate) suffix: &'static str,
    /// The `scope`, none when empty.
    pub(crate) scope: &'static [&'static str],
    /// The `name`.
    pub(crate) name: &'static str,
    /// The `availableSpotNumber`.
    pub(crate) available: i64,
    /// The `reliability` of `availableSpotNumber`.
    pub(crate) reliability: f64,
    /// The camera suffix that provides `availableSpotNumber`.
    pub(crate) camera: &'static str,
    /// The `totalSpotNumber`.
    pub(crate) total: i64,
    /// The longitude and latitude of `location`.
    pub(crate) position: (f64, f64),
}

/// Downtown1 and Downtown2 of C.2.3, at the [-8.5, 41.2] and [-8.51, 41.1] the annex prints.
pub(crate) const PARKINGS: [Parking; 2] = [
    Parking {
        suffix: "Downtown1",
        scope: &[],
        name: "Downtown One",
        available: 121,
        reliability: 0.7,
        camera: "C1",
        total: 200,
        position: (-8.5, 41.2),
    },
    Parking {
        suffix: "Downtown2",
        scope: &[],
        name: "Downtown Two",
        available: 99,
        reliability: 0.8,
        camera: "C2",
        total: 100,
        position: (-8.51, 41.1),
    },
];

/// The time the annex observes every `availableSpotNumber` at.
const OBSERVED_AT: &str = "2017-07-29T12:05:02Z";

/// The parking `parking` as an Entity to create.
fn parking_entity(namespace: &Namespace, parking: &Parking) -> Result<Entity<Input>> {
    let available = Property::builder(parking.available)
        .observed_at(OBSERVED_AT.parse::<DateTime>()?)
        .property("reliability", Value::try_from(parking.reliability)?)
        .relationship("providedBy", namespace.urn("Camera", parking.camera)?);
    let scopes = parking.scope.iter().map(|scope| scope.parse::<Scope>()).collect::<Result<Vec<_>, _>>()?;
    let builder = Entity::builder()
        .id(namespace.urn("OffStreetParking", parking.suffix)?)
        .entity_type(namespace.entity_type("OffStreetParking")?);
    let scope = if scopes.is_empty() { None } else { Some(OneOrMany::try_from(scopes)?) };
    let builder = builder.maybe_scope(scope);
    Ok(builder
        .property("name", parking.name)
        .property("availableSpotNumber", available)
        .property("totalSpotNumber", parking.total)
        .geo_property("location", Shape::Point(Position::try_new(parking.position.0, parking.position.1)?))
        .build()?)
}

/// Creates `parkings` in one batch through `client`, requiring 201.
pub(crate) async fn create_parkings(client: &BrokerClient, namespace: &Namespace, parkings: &[Parking]) -> Result<()> {
    let parkings = parkings.iter().map(|parking| parking_entity(namespace, parking)).collect::<Result<Vec<_>>>()?;
    let created = client.with_responses().entities().batch_create(&parkings).send().await;
    verify_that!(&created, answered(StatusCode::CREATED))?;
    Ok(())
}

/// The point at `parking`'s position.
fn point(parking: &Parking) -> Json {
    json!({"type": "Point", "coordinates": [parking.position.0, parking.position.1]})
}

/// The attributes of `parking` in `representation`, without id and type.
fn attributes(namespace: &Namespace, parking: &Parking, representation: Representation) -> Json {
    let camera = namespace.urn_text("Camera", parking.camera);
    match representation {
        Representation::Normalized => json!({
            "name": {"type": "Property", "value": parking.name},
            "availableSpotNumber": {
                "type": "Property",
                "value": parking.available,
                "observedAt": OBSERVED_AT,
                "reliability": {"type": "Property", "value": parking.reliability},
                "providedBy": {"type": "Relationship", "object": camera},
            },
            "totalSpotNumber": {"type": "Property", "value": parking.total},
            "location": {"type": "GeoProperty", "value": point(parking)},
        }),
        Representation::Concise => json!({
            "name": parking.name,
            "availableSpotNumber": {
                "value": parking.available,
                "observedAt": OBSERVED_AT,
                "reliability": parking.reliability,
                "providedBy": {"object": camera},
            },
            "totalSpotNumber": parking.total,
            "location": point(parking),
        }),
        Representation::Simplified => json!({
            "name": parking.name,
            "availableSpotNumber": parking.available,
            "totalSpotNumber": parking.total,
            "location": point(parking),
        }),
    }
}

/// Downtown1 as an Entity in `representation`, without its `@context`.
pub(crate) fn downtown_one(namespace: &Namespace, representation: Representation) -> Json {
    entity(namespace, &PARKINGS[0], representation)
}

/// `parking` as an Entity in `representation`, without an `@context`.
pub(crate) fn entity(namespace: &Namespace, parking: &Parking, representation: Representation) -> Json {
    let mut members = Map::from_iter([
        ("id".to_owned(), json!(namespace.urn_text("OffStreetParking", parking.suffix))),
        ("type".to_owned(), json!(namespace.suffixed("OffStreetParking"))),
    ]);
    match parking.scope {
        [] => {}
        [scope] => {
            members.insert("scope".to_owned(), json!(scope));
        }
        scopes => {
            members.insert("scope".to_owned(), json!(scopes));
        }
    }
    if let Json::Object(attributes) = attributes(namespace, parking, representation) {
        members.extend(attributes);
    }
    Json::Object(members)
}

/// `parking` as a `GeoJSON` Feature in `representation`, without an `@context`.
fn feature(namespace: &Namespace, parking: &Parking, representation: Representation) -> Json {
    let mut properties = Map::from_iter([("type".to_owned(), json!(namespace.suffixed("OffStreetParking")))]);
    if let Json::Object(attributes) = attributes(namespace, parking, representation) {
        properties.extend(attributes);
    }
    json!({
        "id": namespace.urn_text("OffStreetParking", parking.suffix),
        "type": "Feature",
        "geometry": point(parking),
        "properties": properties,
    })
}

/// Downtown1 as a `GeoJSON` Feature in `representation`, without its `@context`.
pub(crate) fn downtown_one_feature(namespace: &Namespace, representation: Representation) -> Json {
    feature(namespace, &PARKINGS[0], representation)
}

/// Both parkings as the Features of a `FeatureCollection` in `representation`, ordered by id.
pub(crate) fn parking_features(namespace: &Namespace, representation: Representation) -> Json {
    Json::Array(PARKINGS.iter().map(|parking| feature(namespace, parking, representation)).collect())
}
