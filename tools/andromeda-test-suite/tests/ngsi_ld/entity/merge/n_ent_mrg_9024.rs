//! A concise value under a term coerced to date time is merged as a typed date time.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    core_context::core_entry,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, headers::Accept, operation::MergeEntity},
    model::{Context, ContextEntry, Entity, EntityFragment, InlineContext, Lossless, NgsiLdMediaType, RawJson},
};

/// Pins: Merge Entity with a concise `testedAt` 2018-12-04T12:00:00.000Z under a user @context coercing `testedAt`
/// to `DateTime` stores a typed `DateTime`, read back as 2018-12-04T12:00:00Z typed, while the normalized
/// `testedAtNote` under the same coercion stays the plain string written
/// Clause: 4.5.2.3, 5.6.17, C.6
/// Recorded: JSON-LD 1.1 value expansion reads a bare scalar under a term coerced to a datatype as a typed literal;
/// the coercion of the attribute term does not reach the value member of a normalized Property
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9024_a_concise_value_under_a_term_coerced_to_date_time_is_merged_as_a_typed_date_time() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_coerced_id = namespace.urn("Vehicle", "coerced")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let body_context = broker.with_context(Context::Multiple(vec![ContextEntry::Inline(format!(r#"{{"testedAt": {{"@id": "https://example.org/{namespace}/testedAt", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}}, "testedAtNote": {{"@id": "https://example.org/{namespace}/testedAtNote", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}}}}"#).parse::<InlineContext>()?), core_entry()?]))?.with_context_delivery(ContextDelivery::LdJsonBody);
    let vehicle_coerced = Entity::builder()
        .id(vehicle_coerced_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 55)
        .build()?;
    broker.entities().create(&vehicle_coerced).send().await?.done()?;
    let fragment = EntityFragment::builder().property("testedAtNote", "2018-12-04T12:00:00.000Z").build()?;
    // The model refuses it (the model reads it as written otherwise [at $.testedAtNote (written {"type":"Property","value":"2018-12-04T12:00:00.000Z"})]: {"testedAt":"2018-12-04T12:00:00.000Z","testedAtNote":"2018-12-04T12:00:00.000Z"}), so `testedAt` is written as text.
    let tested_at: RawJson = r#""2018-12-04T12:00:00.000Z""#.parse()?;
    let document = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("testedAt", &tested_at);
    let request = body_context
        .unchecked(&MergeEntity::builder().id(&vehicle_coerced_id).fragment(&fragment).build())?
        .document(&document);
    let answer = body_context.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::NO_CONTENT));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&vehicle_coerced_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            &format!("https://example.org/{namespace}/testedAt"),
            any_attribute()
                .value_member(["type"], "DateTime")
                .value_member(["@value"], "2018-12-04T12:00:00Z")
        )
    );
    expect_that!(
        entity,
        has_attribute(
            &format!("https://example.org/{namespace}/testedAtNote"),
            any_attribute().value("2018-12-04T12:00:00.000Z")
        )
    );
    Ok(())
}
