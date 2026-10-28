//! ETSI `041_03`: a query pages through the Context Source Registration Subscriptions of the tenant, and the `Link`
//! field of each page names the pages before and after it (clause 5.11.5).

use crate::suite_keywords::{LD_JSON, PageLink, create_registration_subscription_triple, expect_page_links, page_link};
use andromeda_test_suite::{broker, namespace::Namespace, notification_sink::NotificationSink, problem::answered, teardown::AnyAnswer, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{http::headers::RelationType, model::NgsiLdMediaType};
use serde_json::{Value, json};

/// Pins: Check that one can query context source registration subscriptions with providing offset and limit
/// parameters for pagination
/// Clause: 5.11.5
/// Corrected: `fix(031_02, 037_11, 041_03)`: compare page links by their limit and offset only (6.3.10, 5.5.9.2)
/// Corrected: `fix(041_03)`: request application/ld+json in the csr-subscription pagination query (6.3.6)
/// Corrected: `fix(037_11)`: retarget csr pagination to real pages and check only spec relations (5.5.9)
/// Changed: Check Pagination Prev And Next Headers reads the Link field as RequestsLibrary hands it over, every field
/// line joined with ", ", splits it on commas, drops the blank items and those holding rel="first", rel="last" or
/// json-ld#context, reduces the query of each remaining Link Target to its limit and offset parameters, and compares
/// the rest, spaces removed, with the expected prev and next links, reduced the same way, as a multiset; the port
/// asserts exactly that with regexes over the field value, which assumes that the response carries at most one Link
/// field line: Hurl 8.0.1 reads a field sent on several lines as a list (runner/query.rs:107-122), and every predicate
/// and filter takes either a string or a list (matches and endsWith are type errors on a list, contains tests list
/// membership, toString refuses a list, runner/filter/to_string.rs:25-40 and runner/value.rs:165-173, and no filter
/// joins one), so a broker that sends its link-values on several Link lines, which RFC 9110 clause 5.3 and RFC 8288
/// clause 3 allow, fails here although the keyword passes it (Andromeda defect D-26 records the one-line shape the
/// port needs)
/// Changed: header "Link" not contains "\\", not contains "'''" and not endsWith "'": the keyword pastes the field
/// value into a Python '''...''' literal, so a backslash is read as an escape and a ''' or a final ' ends the literal;
/// the port refuses all three, which is stricter only where an escape leaves the keyword reading, and accepting, a
/// value other than the one sent
/// Changed: Hurl trims the whitespace around a field value (http/client.rs:902), so an item ending in a HTAB at the
/// end of the field, which the keyword compares with the HTAB kept, reads here without it; RFC 9110 clause 5.5 puts
/// that whitespace outside the field value
/// Tenant: private (Query Context Source Registration Subscriptions pages through every context source registration
/// subscription of the tenant, and the case checks how many a limit and an offset let through and the prev and next
/// links that follow from the tenant's total; in the shared tenant every other file's subscriptions are paged too, so
/// only a tenant of its own keeps the page and its links exact)
#[gtest]
#[tokio::test]
async fn e041_03_01_query_second_subscription() -> Result<()> {
    paged(
        1,
        1,
        expect_one_subscription,
        &[
            page_link("/ngsi-ld/v1/csourceSubscriptions?limit=1&offset=0", RelationType::Prev, LD_JSON)?,
            page_link("/ngsi-ld/v1/csourceSubscriptions?limit=1&offset=2", RelationType::Next, LD_JSON)?,
        ],
    )
    .await
}

/// Pins: Check that one can query context source registration subscriptions with providing offset and limit
/// parameters for pagination
/// Clause: 5.11.5
/// Corrected: `fix(031_02, 037_11, 041_03)`: compare page links by their limit and offset only (6.3.10, 5.5.9.2)
/// Corrected: `fix(041_03)`: request application/ld+json in the csr-subscription pagination query (6.3.6)
/// Corrected: `fix(037_11)`: retarget csr pagination to real pages and check only spec relations (5.5.9)
/// Changed: Check Pagination Prev And Next Headers reads the Link field as RequestsLibrary hands it over, every field
/// line joined with ", ", splits it on commas, drops the blank items and those holding rel="first", rel="last" or
/// json-ld#context, reduces the query of each remaining Link Target to its limit and offset parameters, and compares
/// the rest, spaces removed, with the expected prev and next links, reduced the same way, as a multiset; the port
/// asserts exactly that with regexes over the field value, which assumes that the response carries at most one Link
/// field line: Hurl 8.0.1 reads a field sent on several lines as a list (runner/query.rs:107-122), and every predicate
/// and filter takes either a string or a list (matches and endsWith are type errors on a list, contains tests list
/// membership, toString refuses a list, runner/filter/to_string.rs:25-40 and runner/value.rs:165-173, and no filter
/// joins one), so a broker that sends its link-values on several Link lines, which RFC 9110 clause 5.3 and RFC 8288
/// clause 3 allow, fails here although the keyword passes it (Andromeda defect D-26 records the one-line shape the
/// port needs)
/// Changed: header "Link" not contains "\\", not contains "'''" and not endsWith "'": the keyword pastes the field
/// value into a Python '''...''' literal, so a backslash is read as an escape and a ''' or a final ' ends the literal;
/// the port refuses all three, which is stricter only where an escape leaves the keyword reading, and accepting, a
/// value other than the one sent
/// Changed: Hurl trims the whitespace around a field value (http/client.rs:902), so an item ending in a HTAB at the
/// end of the field, which the keyword compares with the HTAB kept, reads here without it; RFC 9110 clause 5.5 puts
/// that whitespace outside the field value
/// Tenant: private (Query Context Source Registration Subscriptions pages through every context source registration
/// subscription of the tenant, and the case checks how many a limit and an offset let through and the prev and next
/// links that follow from the tenant's total; in the shared tenant every other file's subscriptions are paged too, so
/// only a tenant of its own keeps the page and its links exact)
#[gtest]
#[tokio::test]
async fn e041_03_02_query_last_subscription() -> Result<()> {
    paged(
        2,
        2,
        expect_one_subscription,
        &[page_link("/ngsi-ld/v1/csourceSubscriptions?limit=2&offset=0", RelationType::Prev, LD_JSON)?],
    )
    .await
}

/// Pins: Check that one can query context source registration subscriptions with providing offset and limit
/// parameters for pagination
/// Clause: 5.11.5
/// Corrected: `fix(031_02, 037_11, 041_03)`: compare page links by their limit and offset only (6.3.10, 5.5.9.2)
/// Corrected: `fix(041_03)`: request application/ld+json in the csr-subscription pagination query (6.3.6)
/// Corrected: `fix(037_11)`: retarget csr pagination to real pages and check only spec relations (5.5.9)
/// Changed: Check Pagination Prev And Next Headers reads the Link field as RequestsLibrary hands it over, every field
/// line joined with ", ", splits it on commas, drops the blank items and those holding rel="first", rel="last" or
/// json-ld#context, reduces the query of each remaining Link Target to its limit and offset parameters, and compares
/// the rest, spaces removed, with the expected prev and next links, reduced the same way, as a multiset; the port
/// asserts exactly that with regexes over the field value, which assumes that the response carries at most one Link
/// field line: Hurl 8.0.1 reads a field sent on several lines as a list (runner/query.rs:107-122), and every predicate
/// and filter takes either a string or a list (matches and endsWith are type errors on a list, contains tests list
/// membership, toString refuses a list, runner/filter/to_string.rs:25-40 and runner/value.rs:165-173, and no filter
/// joins one), so a broker that sends its link-values on several Link lines, which RFC 9110 clause 5.3 and RFC 8288
/// clause 3 allow, fails here although the keyword passes it (Andromeda defect D-26 records the one-line shape the
/// port needs)
/// Changed: header "Link" not contains "\\", not contains "'''" and not endsWith "'": the keyword pastes the field
/// value into a Python '''...''' literal, so a backslash is read as an escape and a ''' or a final ' ends the literal;
/// the port refuses all three, which is stricter only where an escape leaves the keyword reading, and accepting, a
/// value other than the one sent
/// Changed: Hurl trims the whitespace around a field value (http/client.rs:902), so an item ending in a HTAB at the
/// end of the field, which the keyword compares with the HTAB kept, reads here without it; RFC 9110 clause 5.5 puts
/// that whitespace outside the field value
/// Tenant: private (Query Context Source Registration Subscriptions pages through every context source registration
/// subscription of the tenant, and the case checks how many a limit and an offset let through and the prev and next
/// links that follow from the tenant's total; in the shared tenant every other file's subscriptions are paged too, so
/// only a tenant of its own keeps the page and its links exact)
#[gtest]
#[tokio::test]
async fn e041_03_03_query_all_subscriptions() -> Result<()> {
    paged(
        15,
        0,
        |body| {
            expect_that!(body.as_array().map(Vec::len), some(eq(3)));
            for pointer in ["/0/type", "/1/type", "/2/type"] {
                expect_that!(body.pointer(pointer), some(eq(&json!("Subscription"))));
            }
        },
        &[],
    )
    .await
}

/// Requires the body to be a list holding exactly one Subscription, or that Subscription as a single object.
fn expect_one_subscription(body: &Value) {
    let listed = body.as_array().map_or_else(|| vec![body], |items| items.iter().collect::<Vec<_>>());
    expect_that!(listed.len(), eq(1));
    expect_that!(
        listed.first().and_then(|subscription| subscription.get("type")),
        some(eq(&json!("Subscription")))
    );
}

/// Sets up the three subscriptions in a tenant of the test's own, queries them with `limit` and `offset` in the
/// vocabulary's `Link` context with `Accept: application/ld+json`, requires 200, checks the body with `check` and the
/// page links against `links`, and deletes the subscriptions.
async fn paged(limit: u32, offset: u32, check: impl FnOnce(&Value), links: &[PageLink]) -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let sink = NotificationSink::start().await?;
    let (vocabulary, ids, _subscriptions) = create_registration_subscription_triple(&tenant, &namespace, &sink.endpoint(1)?).await?;

    let received = tenant
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .registration_subscriptions()
        .query()
        .limit(limit)
        .offset(offset)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let received = received?;
    check(&json_body(&received)?);
    expect_page_links(received.head.headers(), links)?;

    for id in &ids {
        tenant.registration_subscriptions().delete(id).send().await.any_answer();
    }
    Ok(())
}
