//! A query type naming a keyword alias is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{ContextDelivery, operation::QueryMethod},
    model::{Context, ContextEntry, ErrorType, InlineContext},
};

/// Pins: Query Entities by POST whose entity type is a term the @context aliases to @type answers 400
/// `BadRequestData`, as a write using the same alias does, not 500
/// Clause: 5.7.2.4, 5.5.7, Table 5.5.2-1
/// Recorded: a keyword alias names no type, so the input "does not meet the requirements of the operation" (Table
/// 5.5.2-1, `BadRequestData`); `InternalError` is for "an error during the operation execution", and nothing failed
/// inside the broker
#[gtest]
#[tokio::test]
async fn n_qry_ent_4001_a_query_type_naming_a_keyword_alias_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let body_context = broker
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(format!(r#"{{"Kind_{namespace}": "@type"}}"#).parse::<InlineContext>()?),
            ContextEntry::Reference(IriRefBuf::new("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld".to_owned())?),
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let answer = body_context
        .with_responses()
        .entities()
        .query()
        .method(QueryMethod::Post)
        .entity_type(&namespace.entity_type("Kind")?)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
