//! ETSI `053_02`: a hosted `@context` served with `details=true` answers its details, in `application/json`, with no
//! member beyond them (clauses 5.13.3.5 and 5.13.4).

use crate::jsonld_context::stored_contexts::{
    add_context,
    attribute_terms,
    details_in_json,
    expect_context_details,
    expect_only_detail_members,
    served_details,
};
use andromeda_test_suite::{broker, namespace::Namespace, teardown::AnyAnswer};
use googletest::prelude::*;
use ngsi_ld::http::operation::ContextKindFilter;

/// Pins: Check that one can serve a @context with details
/// Clause: 5.13.4
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: lastUsage and numberOfHits are required to be absent or of their type (a `DateTime` and a number, clause
/// 5.13.3.5), where the keyword also skips a null or empty value
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e053_02_01_serve_a_context_with_details_set_to_true() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let added = add_context(&broker, &attribute_terms(&namespace, &["A1", "A2", "A3"])?).await?;
    let details = details_in_json(&served_details(&broker, &added.id).await?)?;
    expect_context_details(&details, ContextKindFilter::Hosted);
    expect_only_detail_members(&details)?;
    broker.contexts().delete(&added.id).send().await.any_answer();
    Ok(())
}
