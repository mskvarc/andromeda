//! Every attribute family reads back its expires at with sys attrs.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{CreateEntity, SystemAttributes},
    },
    model::{
        Entity,
        GeoProperty,
        JsonProperty,
        LanguageMap,
        LanguageProperty,
        ListProperty,
        Lossless,
        NgsiLdMediaType,
        Property,
        RawJson,
        Relationship,
        Value,
        VocabProperty,
        VocabTerm,
        geometry::{Position, Shape},
    },
};

/// Pins: an Attribute of each of the eight families written with its own `expiresAt` reads back with that
/// `expiresAt` when options holds `sysAttrs`
/// Clause: 4.22, 6.3.11, 5.7.1.4
#[gtest]
#[tokio::test]
async fn n_ent_ret_0039_every_attribute_family_reads_back_its_expires_at_with_sys_attrs() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let parking_spot_id = namespace.urn("ParkingSpot", 1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(80).expires_at("2099-12-31T23:59:59Z".parse()?))
        .relationship("isParkedIn", Relationship::builder(parking_spot_id).expires_at("2099-06-15T12:00:00Z".parse()?))
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_new(13.0, 52.0)?)).expires_at("2099-01-01T00:00:00Z".parse()?),
        )
        .vocab_property(
            "category",
            VocabProperty::builder("bus".parse::<VocabTerm>()?).expires_at("2099-03-15T08:00:00Z".parse()?),
        )
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "bus"), ("de", "Bus")])?).expires_at("2099-07-01T00:00:00Z".parse()?),
        )
        .json_property(
            "diagnostics",
            JsonProperty::builder(r#"{"engine":"ok","brakes":"warning"}"#.parse::<RawJson>()?).expires_at("2099-09-01T12:00:00Z".parse()?),
        )
        .list_property(
            "tags",
            ListProperty::builder(vec![Value::from("a"), Value::from("b")]).expires_at("2099-11-20T06:30:00Z".parse()?),
        )
        .build()?;
    // The model refuses it (the model reads it as written otherwise [at $.connectedTo.objectList[0] (written "urn:ngsi-ld:Vehicle:zqns-2")]: {"id":"urn:ngsi-ld:Vehicle:zqns-1","type":"Vehicle_zqns","speed":{"type":"Property","value":80,"expiresAt":"2099-12-31T23:59:59Z"},"isParkedIn":{"type":"Relationship","object":"urn:ngsi-ld:ParkingSpot:zqns-1","expiresAt":"2099-06-15T12:00:00Z"},"location":{"type":"GeoProperty","value":{"type":"Point","coordinates":[13.0,52.0]},"expiresAt":"2099-01-01T00:00:00Z"},"category":{"type":"VocabProperty","vocab":"bus","expiresAt":"2099-03-15T08:00:00Z"},"name":{"type":"LanguageProperty","languageMap":{"en":"bus","de":"Bus"},"expiresAt":"2099-07-01T00:00:00Z"},"diagnostics":{"type":"JsonProperty","json":{"engine":"ok","brakes":"warning"},"expiresAt":"2099-09-01T12:00:00Z"},"tags":{"type":"ListProperty","valueList":["a","b"],"expiresAt":"2099-11-20T06:30:00Z"},"connectedTo":{"type":"ListRelationship","objectList":[{"object":"urn:ngsi-ld:Vehicle:zqns-2"},{"object":"urn:ngsi-ld:Vehicle:zqns-3"}],"expiresAt":"2099-04-10T18:00:00Z"}}), so `connectedTo` is written as text.
    let connected_to: RawJson = format!(r#"{{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Vehicle:{namespace}-2", "urn:ngsi-ld:Vehicle:{namespace}-3"], "expiresAt": "2099-04-10T18:00:00Z"}}"#).parse()?;
    let document = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized).set_member("connectedTo", &connected_to);
    let request = broker.unchecked(&CreateEntity::builder().entity(&vehicle).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_id)
        .system_attributes(SystemAttributes::Include)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("speed", any_attribute().expires_at("2099-12-31T23:59:59Z".parse()?)));
    expect_that!(entity, has_attribute("isParkedIn", any_attribute().expires_at("2099-06-15T12:00:00Z".parse()?)));
    expect_that!(entity, has_attribute("location", any_attribute().expires_at("2099-01-01T00:00:00Z".parse()?)));
    expect_that!(entity, has_attribute("category", any_attribute().expires_at("2099-03-15T08:00:00Z".parse()?)));
    expect_that!(entity, has_attribute("name", any_attribute().expires_at("2099-07-01T00:00:00Z".parse()?)));
    expect_that!(
        entity,
        has_attribute("diagnostics", any_attribute().expires_at("2099-09-01T12:00:00Z".parse()?))
    );
    expect_that!(entity, has_attribute("tags", any_attribute().expires_at("2099-11-20T06:30:00Z".parse()?)));
    expect_that!(
        entity,
        has_attribute("connectedTo", any_attribute().expires_at("2099-04-10T18:00:00Z".parse()?))
    );
    Ok(())
}
