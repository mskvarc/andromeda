//! ETSI `053_04`: the `numberOfHits` of a hosted `@context` counts the requests that use it (clauses 5.13.3.5 and
//! 5.13.4).

use crate::jsonld_context::{
    stored_contexts::{add_context, attribute_terms, served_details},
    testing_entity::testing_entity,
};
use andromeda_test_suite::{broker, namespace::Namespace, teardown::AnyAnswer, wire};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, operation::CreateEntity},
    model::Representation,
};
use serde_json::{Value, json};

/// Pins: Check that the numberOfHits is increased after using a Hosted context
/// Clause: 5.13.4
/// Changed: numberOfHits is required to be absent or the expected number, where the keyword also skips a null or empty
/// value and compares a text as a number; clause 5.13.3.5 makes it a number
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e053_04_01_number_of_hits_increases_after_using_a_hosted_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let added = add_context(&broker, &attribute_terms(&namespace, &["A1", "A2", "A3"])?).await?;
    let before = serde_json::from_slice::<Value>(served_details(&broker, &added.id).await?.body())?;
    expect_that!(before.get("numberOfHits"), any![none(), some(eq(&json!(0)))]);
    let testing = testing_entity(&namespace, "randomUUID")?;
    // Unchecked: the body names the stored context by its relative `Location`, which a typed `@context` refuses.
    let writer = broker.with_context_delivery(ContextDelivery::LdJsonBody);
    let document = UncheckedDocument::try_from(testing.render(Representation::Normalized))?.set_text("@context", added.location.as_str());
    let request = writer
        .unchecked(&CreateEntity::builder().entity(&testing).build())?
        .uri(wire::api_uri(&writer, "entities/")?)
        .document(&document);
    writer.send_unchecked(request).await.any_answer();
    let after = serde_json::from_slice::<Value>(served_details(&broker, &added.id).await?.body())?;
    expect_that!(after.get("numberOfHits"), any![none(), some(eq(&json!(1)))]);
    let building = namespace.urn("Building", "randomUUID")?;
    broker.entities().delete(&building).send().await.any_answer();
    broker.temporal().delete(&building).send().await.any_answer();
    broker.contexts().delete(&added.id).send().await.any_answer();
    Ok(())
}
