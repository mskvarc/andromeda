//! Clauses C.5.2, C.5.3, C.5.4, C.5.13 and C.5.15: the entity-facing REST examples.
//!
//! Each test issues the printed HTTP request against the seeded tenant and
//! compares the printed response.
//!
//! Two of the printed exchanges cannot be replayed exactly as printed and are
//! adjusted. C.5.3's
//! request carries no `attrs`, yet its answer shows only `brandName`, so the
//! fixtures give each Vehicle only that Attribute. C.5.4 asks for the first
//! page of two and its answer carries a `next` link, which needs a third
//! Vehicle the annex does not print.

use crate::{
    BASE_URL,
    clean_tenant,
    comparison::{ContextMember, EntityOrder, assert_matches_example, corpus},
    context_link,
    create_entity,
    create_entity_batch,
    example_context::hosted_contexts,
    get_json,
    get_with_headers,
};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

#[tokio::test]
async fn annex_c_5_2_create_entity_of_type_vehicle() {
    let environment = clean_tenant().await;
    let link = context_link(hosted_contexts().url("aggregatedContext.jsonld").as_str());

    let response = environment
        .client()
        .post(format!("{BASE_URL}/entities/"))
        .header("Content-Type", "application/json")
        .header("Link", &link)
        .json(&crate::without_context(&corpus(include_str!("corpus/c-2-2-normalized-unitless.json"))))
        .send()
        .await
        .expect("entity creation request");

    let status = response.status();
    let location = response
        .headers()
        .get("Location")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let body = response.text().await.unwrap_or_default();

    assert_eq!(status, StatusCode::CREATED, "the C.5.2 creation was refused: {body}");
    assert_eq!(
        location, "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:A4567",
        "clause C.5.2.2 prints this exact Location header"
    );

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_5_3_query_entities_example() {
    let environment = clean_tenant().await;
    let link = context_link(hosted_contexts().url("aggregatedContext.jsonld").as_str());
    create_entity_batch(&environment.client(), &corpus(include_str!("corpus/c-5-3-vehicles-seed.json")), Some(&link)).await;

    let (status, actual) = get_json(
        &environment.client(),
        "/entities/?type=Vehicle&q=brandName!=%22Mercedes%22&format=simplified",
        "application/ld+json",
        Some(&link),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the C.5.3 query failed: {actual}");

    let expected = corpus(include_str!("corpus/c-5-3-query-answer.json"));
    assert_matches_example("C.5.3", &expected, &actual, ContextMember::Ignored, EntityOrder::Unordered);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_5_4_query_entities_pagination_example() {
    let environment = clean_tenant().await;
    let link = context_link(hosted_contexts().url("aggregatedContext.jsonld").as_str());
    create_entity_batch(&environment.client(), &corpus(include_str!("corpus/c-5-3-vehicles-seed.json")), Some(&link)).await;

    let (status, headers, actual) = get_with_headers(
        &environment.client(),
        "/entities/?type=Vehicle&format=simplified&limit=2",
        "application/ld+json",
        Some(&link),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "the C.5.4 query failed: {actual}");

    // The page itself: two Entities, each rendered as clause C.5.4.3 prints
    // them. Which two is not asserted -- clause 5.7.2 mandates no order for a
    // query without `orderBy`, so the annex's choice of B9211 and A456 is one
    // admissible page of three seeded Vehicles rather than the only one.
    let page = actual.as_array().expect("a query answer is an array");
    assert_eq!(page.len(), 2, "clause C.5.4.2 limits the page to two Entities");
    for entity in page {
        let members = entity.as_object().expect("each page member is an Entity");
        let mut names: Vec<&str> = members.keys().map(String::as_str).filter(|name| *name != "@context").collect();
        names.sort_unstable();
        assert_eq!(names, ["brandName", "id", "type"], "clause C.5.4.3 prints exactly these members");
        assert!(
            entity["brandName"].is_string(),
            "the simplified representation collapses brandName to its value"
        );
    }

    // Clause C.5.4.3 prints the `next` pointer in its transparent `offset`
    // spelling; clause 5.5.9.2 makes that spelling one option beside the
    // opaque pointer of clause 5.5.9.1, which the broker offers on a query in
    // the default order. What the example shows either way is a pointer to the
    // page that follows the two Entities already returned, so the pointer is
    // followed and must answer the third seeded Vehicle alone.
    let next = headers
        .get_all("Link")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .find(|value| value.contains("rel=\"next\""))
        .expect("clause C.5.4.3 prints a next-page Link header");
    let target = next
        .split_once('<')
        .and_then(|(_, rest)| rest.split_once('>'))
        .map(|(target, _)| target)
        .expect("the next pointer carries a target");
    let query = target.split_once('?').map(|(_, query)| query).expect("the next pointer carries the query");
    let (status, _, following) = get_with_headers(&environment.client(), &format!("/entities/?{query}"), "application/ld+json", Some(&link)).await;
    assert_eq!(status, StatusCode::OK, "the next page failed: {following}");
    let ids = |entities: &serde_json::Value| -> Vec<String> {
        entities
            .as_array()
            .expect("a query answer is an array")
            .iter()
            .map(|entity| entity["id"].as_str().expect("an Entity has an id").to_owned())
            .collect()
    };
    let (first, rest) = (ids(&actual), ids(&following));
    assert_eq!(rest.len(), 1, "three seeded Vehicles leave one for the next page: {rest:?}");
    assert!(
        !first.contains(&rest[0]),
        "the next page starts after the two already returned: {first:?} then {rest:?}"
    );

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_5_13_query_entities_with_natural_language_filtering() {
    let environment = clean_tenant().await;
    let link = context_link(hosted_contexts().url("aggregatedContext.jsonld").as_str());
    create_entity(&environment.client(), &corpus(include_str!("corpus/c-5-13-marque-seed.json")), Some(&link)).await;

    // The printed query filters on `marque[en-GB]`. Clause 4.9's ABNF derives
    // the bracketed selector from `AttrName = unicodeLetter *TermChar`, and
    // `TermChar` admits only letters, digits and `_` -- so no BCP 47 tag with a
    // subtag separator can be written there, and the printed query is outside
    // the grammar the same clause mandates. The example is replayed with the
    // `en` the grammar does admit, and the refusal of `en-GB` is asserted
    // separately so the conflict is on the record.
    let query = "/entities/?type=Vehicle&attrs=marque&q=marque%5Ben%5D%3D%3D%22Vauxhall%20Viva%22&format=simplified&lang=de";
    let (status, actual) = get_json(&environment.client(), query, "application/ld+json", Some(&link)).await;
    assert_eq!(status, StatusCode::OK, "the C.5.13 query failed: {actual}");

    let expected = corpus(include_str!("corpus/c-5-13-query-answer.json"));
    assert_matches_example("C.5.13", &expected, &actual, ContextMember::Ignored, EntityOrder::Unordered);

    let printed = "/entities/?type=Vehicle&attrs=marque&q=marque%5Ben-GB%5D%3D%3D%22Vauxhall%20Viva%22&format=simplified&lang=de";
    let (status, refusal) = get_json(&environment.client(), printed, "application/ld+json", Some(&link)).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "clause 4.9's ABNF cannot express `en-GB` in the bracketed selector, got {refusal}"
    );

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_5_15_scope_queries_example() {
    let environment = clean_tenant().await;
    let link = context_link(hosted_contexts().url("aggregatedContext.jsonld").as_str());
    create_entity_batch(&environment.client(), &corpus(include_str!("corpus/c-5-15-scope-seed.json")), Some(&link)).await;

    let query = "/entities/?type=OffStreetParking&scopeQ=%2FMadrid%2FCentro,%2FMadrid%2FCortes";
    let (status, actual) = get_json(&environment.client(), query, "application/ld+json", Some(&link)).await;
    assert_eq!(status, StatusCode::OK, "the C.5.15 query failed: {actual}");

    let expected = corpus(include_str!("corpus/c-5-15-query-answer.json"));
    assert_matches_example("C.5.15", &expected, &actual, ContextMember::Ignored, EntityOrder::Unordered);

    environment.finish().await;
}
