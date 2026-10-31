//! Strings merged under lang replace every stored string of that language.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
    broker,
    namespace::Namespace,
    problem::answered,
    wire::request_to,
};
use bytes::Bytes;
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::CONTENT_TYPE};
use ngsi_ld::{
    http::{headers::Accept, operation::RetrieveEntity},
    model::{Lossless, NgsiLdMediaType},
};

/// Pins: Merge Entity with lang=en of a street whose languageMap is the string array
/// `["Rue Cler", "Rue Saint-Dominique"]` into a stored street with en `["Champ de Mars", "Quai Branly"]` and fr
/// "Champ de Mars" answers 204, and street then holds en `["Rue Cler", "Rue Saint-Dominique"]` beside the untouched fr
/// Clause: 5.6.17.4, 5.5.12, 6.5.3.4
#[gtest]
#[tokio::test]
async fn n_ent_mrg_9027_strings_merged_under_lang_replace_every_stored_string_of_that_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_lang_id = namespace.urn("Building", "lang")?;
    // The model refuses a language map entry holding several strings, so the bodies are sent as written.
    let request = request_to(&broker, Method::POST, "entities")?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from(format!(
            "{{\"id\": \"{building_lang_id}\", \"type\": \"Building_{namespace}\", \"street\": {{\"type\": \"LanguageProperty\", \"languageMap\": {{\"en\": [\"Champ de Mars\", \"Quai Branly\"], \"fr\": \"Champ de Mars\"}}}}}}"
        )));
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let request_2 = request_to(&broker, Method::PATCH, &format!("entities/{building_lang_id}?lang=en"))?
        .append_header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .body(Bytes::from_static(
            b"{\"street\": {\"type\": \"LanguageProperty\", \"languageMap\": [\"Rue Cler\", \"Rue Saint-Dominique\"]}}",
        ));
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::NO_CONTENT));
    // The entity reader refuses the stored language map entry of several strings, so the answer is read as received.
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = json
        .send_unchecked(json.unchecked(&RetrieveEntity::builder().id(&building_lang_id).format(Lossless::Normalized).build())?)
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, json_count("$.street.languageMap.en", 2));
    expect_that!(entity, json_equals("$.street.languageMap.en[0]", "Rue Cler"));
    expect_that!(entity, json_equals("$.street.languageMap.en[1]", "Rue Saint-Dominique"));
    expect_that!(entity, json_equals("$.street.languageMap.fr", "Champ de Mars"));
    Ok(())
}
