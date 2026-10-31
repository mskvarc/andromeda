//! An entity id with a space a tab or a line break is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, ErrorType, Lossless, NgsiLdMediaType},
};

/// Pins: Create Entity whose id holds a literal space, tab or line break answers 400 `BadRequestData` each time,
/// and no Entity is stored under the id with that character removed
/// Clause: 4.5.3.2, 5.2.4, 5.6.1.4; IETF RFC 3986 section 2 and appendix A
#[gtest]
#[tokio::test]
async fn n_cmn_val_9027_an_entity_id_with_a_space_a_tab_or_a_line_break_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let city_id = namespace.urn("City", "whitespace-target")?;
    let placeholder = Entity::builder()
        .id(namespace.urn("Building", "whitespace")?)
        .entity_type(building_type)
        .relationship("locatedAt", city_id)
        .build()?;
    let cases = [
        ("has-space whitespace", "has-spacewhitespace"),
        ("has-tab\twhitespace", "has-tabwhitespace"),
        ("has-line-break\nwhitespace", "has-line-breakwhitespace"),
    ];
    for (spelled, stripped) in cases {
        // The model refuses an identifier holding white space, so `id` is set on the document.
        let document =
            UncheckedDocument::from_entity(&placeholder, Lossless::Normalized).set_text("id", &format!("urn:ngsi-ld:Building:{namespace}-{spelled}"));
        let request = broker.unchecked(&CreateEntity::builder().entity(&placeholder).build())?.document(&document);
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
        let answer_2 = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&namespace.urn("Building", stripped)?)
            .send()
            .await;
        expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    }
    Ok(())
}
