//! An objectList entry naming no URI or holding a second member is refused with the reason.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Create Entity with a `ListRelationship` whose `objectList` holds the bare string "not a uri", a
/// Relationship Object whose `object` is "not a uri", or a Relationship Object holding `id` beside `object` answers
/// 400 `BadRequestData` whose detail names `objectList` and the reason, carries no decoder vocabulary, and stores
/// nothing
/// Clause: 4.5.22.2, 5.2.37, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9061_an_object_list_entry_naming_no_uri_or_holding_a_second_member_is_refused_with_the_reason() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let route_type = namespace.entity_type("Route")?;
    let target = namespace.urn("Stop", 1)?;
    let crowded = format!(r#"{{"type": "ListRelationship", "objectList": [{{"object": "{target}", "id": "{target}"}}]}}"#);
    for (key, written, reason) in [
        (
            "bare",
            r#"{"type": "ListRelationship", "objectList": ["not a uri"]}"#.to_owned(),
            "invalid URI: not a valid URN, URL or IRI",
        ),
        (
            "wrapped",
            r#"{"type": "ListRelationship", "objectList": [{"object": "not a uri"}]}"#.to_owned(),
            "invalid URI: not a valid URN, URL or IRI",
        ),
        ("crowded", crowded, "holds 'id' beside 'object'"),
    ] {
        let route_id = namespace.urn("Route", key)?;
        let route = Entity::builder().id(route_id.clone()).entity_type(route_type.clone()).build()?;
        // The model refuses it (an objectList entry names one URI), so `stops` is written as text.
        let stops: RawJson = written.parse()?;
        let document = UncheckedDocument::from_entity(&route, Lossless::Normalized).set_member("stops", &stops);
        let request = broker.unchecked(&CreateEntity::builder().entity(&route).build())?.document(&document);
        let answer = broker.send_unchecked(request).await;
        expect_that!(
            &answer,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
                .with_detail_containing("'objectList'")
                .with_detail_containing(reason)
                .with_detail_lacking("untagged enum")
                .with_detail_lacking("variant")
                .with_detail_lacking("ObjectListEntry"),
            "objectList {key}"
        );
        let retrieved = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&route_id)
            .send()
            .await;
        expect_that!(&retrieved, answered(StatusCode::NOT_FOUND), "objectList {key}");
    }
    Ok(())
}
