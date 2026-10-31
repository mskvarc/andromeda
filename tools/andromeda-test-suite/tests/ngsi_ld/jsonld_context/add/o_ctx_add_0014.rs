//! Add the same context twice stores two randomly identified entries.

use andromeda_test_suite::{broker, context_details, hosted_context::HostedContext, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::model::{Context, ContextEntry, InlineContext};

/// Pins: Add @context of the same document twice answers 201 twice with the local relative path of two different
/// version 4 identifiers, each of which Serve @context details names as its `localId`
/// Clause: 5.13.2.4, 6.29.3.1, 5.13.3.5
/// Recorded: clause 5.13.1 leaves the format of a stored @context's local identifier to the broker; Andromeda names
/// every entry `urn:ngsi-ld:JsonLdContext:<uuid>`, a random (version 4) UUID for a Hosted or Cached entry and the
/// version 5 UUID of the canonical content for an `ImplicitlyCreated` one
#[gtest]
#[tokio::test]
async fn o_ctx_add_0014_add_the_same_context_twice_stores_two_randomly_identified_entries() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Spot": "https://test.andromeda/{namespace}/Spot"}}"#).parse::<InlineContext>()?,
    ));
    let first = HostedContext::add(&broker, &context).await?;
    expect_that!(first.location, matches_regex(r"^/ngsi-ld/v1/jsonldContexts/[^/?]+$"));
    let second = HostedContext::add(&broker, &context).await?;
    expect_that!(second.location, matches_regex(r"^/ngsi-ld/v1/jsonldContexts/[^/?]+$"));
    expect_that!(second.location, not(eq(&first.location)));
    for stored in [&first, &second] {
        let record = context_details::metadata(&context_details::serve_details(&broker, &stored.id).await?)?;
        expect_that!(record.local_id, eq(&stored.id));
        expect_that!(stored.location, eq(&format!("/ngsi-ld/v1/jsonldContexts/{}", record.local_id)));
        expect_that!(
            record.local_id.to_string(),
            matches_regex(r"^urn:ngsi-ld:JsonLdContext:[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")
        );
    }
    Ok(())
}
