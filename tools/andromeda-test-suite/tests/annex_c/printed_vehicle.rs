//! The normalized Vehicle of Annex C.2.2 and the Entities its Relationships target as the annex prints them, without
//! their `@context`: the documents the C.2.2 retrievals are compared with, and each variant starts from.

use andromeda_test_suite::namespace::Namespace;
use googletest::prelude::*;
use serde_json::{Value, json};

/// The id `urn:ngsi-ld:<kind>:<ns>-<suffix>` as text.
fn id(namespace: &Namespace, kind: &str, suffix: &str) -> String {
    namespace.urn_text(kind, suffix)
}

/// The normalized Vehicle A4567, its `tyreTreadDepths` without a unit.
pub(crate) fn normalized_vehicle(namespace: &Namespace) -> Value {
    json!({
        "id": id(namespace, "Vehicle", "A4567"),
        "type": namespace.suffixed("Vehicle"),
        "brandName": {"type": "Property", "value": "Mercedes"},
        "street": {"type": "LanguageProperty", "languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}},
        "isParked": {
            "type": "Relationship",
            "objectType": namespace.suffixed("OffStreetParking"),
            "object": id(namespace, "OffStreetParking", "Downtown1"),
            "observedAt": "2017-07-29T12:00:04Z",
            "providedBy": {"type": "Relationship", "object": id(namespace, "Person", "Bob")},
        },
        "category": {"type": "VocabProperty", "vocab": "non-commercial"},
        "tyreTreadDepths": {"type": "ListProperty", "valueList": [300, 300, 120, 6], "valueType": "Integer"},
        "passengers": {
            "type": "Relationship",
            "objectType": namespace.suffixed("Person"),
            "object": [id(namespace, "Person", "Alice"), id(namespace, "Person", "Bob")],
        },
        "route": {
            "type": "ListRelationship",
            "objectType": namespace.suffixed("City"),
            "objectList": [
                {"object": id(namespace, "City", "Antwerp")},
                {"object": id(namespace, "City", "Rotterdam")},
                {"object": id(namespace, "City", "Amsterdam")},
            ],
        },
    })
}

/// The normalized parking Downtown1, operated by `BigParkingCorp`.
pub(crate) fn normalized_parking(namespace: &Namespace) -> Value {
    json!({
        "id": id(namespace, "OffStreetParking", "Downtown1"),
        "type": namespace.suffixed("OffStreetParking"),
        "name": {"type": "Property", "value": "Top Parking"},
        "operatedBy": {"type": "Relationship", "object": id(namespace, "Company", "BigParkingCorp")},
    })
}

/// The normalized `<kind>` `<suffix>` whose `name` is `<suffix>`: a person or a city of the example.
pub(crate) fn normalized_named(namespace: &Namespace, kind: &str, suffix: &str) -> Value {
    json!({
        "id": id(namespace, kind, suffix),
        "type": namespace.suffixed(kind),
        "name": {"type": "Property", "value": suffix},
    })
}

/// The persons Alice and Bob, normalized.
pub(crate) fn normalized_persons(namespace: &Namespace) -> Value {
    json!([normalized_named(namespace, "Person", "Alice"), normalized_named(namespace, "Person", "Bob")])
}

/// The cities of the route in its order, normalized.
pub(crate) fn normalized_cities(namespace: &Namespace) -> Value {
    json!([
        normalized_named(namespace, "City", "Antwerp"),
        normalized_named(namespace, "City", "Rotterdam"),
        normalized_named(namespace, "City", "Amsterdam"),
    ])
}

/// The concise Vehicle A4567.
pub(crate) fn concise_vehicle(namespace: &Namespace) -> Value {
    json!({
        "id": id(namespace, "Vehicle", "A4567"),
        "type": namespace.suffixed("Vehicle"),
        "brandName": "Mercedes",
        "street": {"languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}},
        "isParked": {
            "object": id(namespace, "OffStreetParking", "Downtown1"),
            "objectType": namespace.suffixed("OffStreetParking"),
            "observedAt": "2017-07-29T12:00:04Z",
            "providedBy": {"object": id(namespace, "Person", "Bob")},
        },
        "category": {"vocab": "non-commercial"},
        "tyreTreadDepths": {"valueList": [300, 300, 120, 6], "valueType": "Integer"},
        "passengers": {
            "objectType": namespace.suffixed("Person"),
            "object": [id(namespace, "Person", "Alice"), id(namespace, "Person", "Bob")],
        },
        "route": {
            "objectType": namespace.suffixed("City"),
            "objectList": [
                id(namespace, "City", "Antwerp"),
                id(namespace, "City", "Rotterdam"),
                id(namespace, "City", "Amsterdam"),
            ],
        },
    })
}

/// The concise parking Downtown1.
pub(crate) fn concise_parking(namespace: &Namespace) -> Value {
    json!({
        "id": id(namespace, "OffStreetParking", "Downtown1"),
        "type": namespace.suffixed("OffStreetParking"),
        "name": "Top Parking",
        "operatedBy": {"object": id(namespace, "Company", "BigParkingCorp")},
    })
}

/// The `<kind>` `<suffix>` whose `name` is `<suffix>`, concise or simplified: both write the name as its bare value.
pub(crate) fn bare_named(namespace: &Namespace, kind: &str, suffix: &str) -> Value {
    json!({"id": id(namespace, kind, suffix), "type": namespace.suffixed(kind), "name": suffix})
}

/// The persons Alice and Bob, concise or simplified.
pub(crate) fn bare_persons(namespace: &Namespace) -> Value {
    json!([bare_named(namespace, "Person", "Alice"), bare_named(namespace, "Person", "Bob")])
}

/// The cities of the route in its order, concise or simplified.
pub(crate) fn bare_cities(namespace: &Namespace) -> Value {
    json!([
        bare_named(namespace, "City", "Antwerp"),
        bare_named(namespace, "City", "Rotterdam"),
        bare_named(namespace, "City", "Amsterdam"),
    ])
}

/// The simplified Vehicle A4567.
pub(crate) fn simplified_vehicle(namespace: &Namespace) -> Value {
    json!({
        "id": id(namespace, "Vehicle", "A4567"),
        "type": namespace.suffixed("Vehicle"),
        "brandName": "Mercedes",
        "street": {"languageMap": {"fr": "Grand Place", "nl": "Grote Markt"}},
        "isParked": id(namespace, "OffStreetParking", "Downtown1"),
        "category": {"vocab": "non-commercial"},
        "tyreTreadDepths": [300, 300, 120, 6],
        "passengers": [id(namespace, "Person", "Alice"), id(namespace, "Person", "Bob")],
        "route": [id(namespace, "City", "Antwerp"), id(namespace, "City", "Rotterdam"), id(namespace, "City", "Amsterdam")],
    })
}

/// The simplified parking Downtown1.
pub(crate) fn simplified_parking(namespace: &Namespace) -> Value {
    json!({
        "id": id(namespace, "OffStreetParking", "Downtown1"),
        "type": namespace.suffixed("OffStreetParking"),
        "name": "Top Parking",
        "operatedBy": id(namespace, "Company", "BigParkingCorp"),
    })
}

/// The flattened answer of the example in one representation: the Vehicle `vehicle`, the parking `parking`, then the
/// persons and cities `named` builds.
pub(crate) fn flattened(namespace: &Namespace, vehicle: Value, parking: Value, named: fn(&Namespace, &str, &str) -> Value) -> Vec<Value> {
    vec![
        vehicle,
        parking,
        named(namespace, "Person", "Alice"),
        named(namespace, "Person", "Bob"),
        named(namespace, "City", "Antwerp"),
        named(namespace, "City", "Rotterdam"),
        named(namespace, "City", "Amsterdam"),
    ]
}

/// Sets member `name` of the object at `pointer` in `document` to `value`, failing when there is no object there.
pub(crate) fn set_member(document: &mut Value, pointer: &str, name: &str, value: Value) -> Result<()> {
    document
        .pointer_mut(pointer)
        .and_then(Value::as_object_mut)
        .or_fail()?
        .insert(name.to_owned(), value);
    Ok(())
}
