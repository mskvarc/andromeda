//! Add discards members outside the context subtree.

use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use indexmap::IndexMap;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, ExtraMemberName, InlineContext, LocalContext, Value},
};

/// Pins: Add @context of a document carrying an id, kind, createdAt, extraInfo and an unrelated member beside its
/// @context answers 201, Serve @context answers the @context subtree alone, and Delete @context answers 204
/// Clause: 5.13.2.3, 5.13.2.4, 5.13.4.4, 5.13.5.4
#[gtest]
#[tokio::test]
async fn n_ctx_add_9001_add_discards_members_outside_the_context_subtree() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Device": "https://example.org/{namespace}/Device", "temperature": "https://example.org/{namespace}/temperature"}}"#)
            .parse::<InlineContext>()?,
    ));
    let members = IndexMap::from([
        (
            "id".parse::<ExtraMemberName>()?,
            Value::from(format!("urn:ngsi-ld:Context:{namespace}-supplied-by-the-client")),
        ),
        ("kind".parse::<ExtraMemberName>()?, Value::from("Hosted")),
        ("createdAt".parse::<ExtraMemberName>()?, Value::from("2020-01-01T00:00:00Z")),
        (
            "extraInfo".parse::<ExtraMemberName>()?,
            Value::Object(IndexMap::from([("owner".to_owned(), Value::from("someone"))])),
        ),
        (
            "unrelated".parse::<ExtraMemberName>()?,
            Value::Array(vec![Value::from(1), Value::from(2), Value::from(3)]),
        ),
    ]);
    let created = broker.contexts().add(&context).extra_members(&members).send().await?;
    let id = created.id().or_fail()?;
    let received = broker.with_responses().with_accept(Accept::any()).contexts().serve(&id).send().await?;
    // The served operation outcome keeps the @context member alone, so the whole body is read to see what it holds.
    let document = LocalContext::parse(str::from_utf8(&received.body)?)?;
    expect_that!(document.extra_members, is_empty());
    let Context::Single(ContextEntry::Inline(terms)) = document.context else {
        return fail!("the served context is not one inline object");
    };
    expect_that!(
        terms.to_string(),
        eq(&format!(
            r#"{{"Device":"https://example.org/{namespace}/Device","temperature":"https://example.org/{namespace}/temperature"}}"#
        ))
    );
    let deleted = broker.with_responses().contexts().delete(&id).send().await?;
    expect_that!(deleted.body.len(), eq(0));
    Ok(())
}
