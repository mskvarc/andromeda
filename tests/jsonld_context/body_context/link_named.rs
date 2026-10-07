//! Every rendering resource, under every request context a `Link` can name.
//!
//! Each case creates one resource of every kind that renders a `@context`
//! (Entity retrieval and query, temporal retrieval, Subscription, Context
//! Source Subscription and Registration retrieval, the entity type and
//! attribute lists, an entity map) and checks each names exactly the rule's
//! value.

use crate::{
    body_context::rendered_resources::{RenderedResources, assert_every_resource_named},
    core_contexts::{CORE_V18, CORE_VERSIONLESS, core_value},
    private_tenant::PrivateTenant,
    user_contexts::UserContexts,
};
use serde_json::{Value, json};

/// Render every resource under the context `link` picks from the served
/// shapes (none when it picks nothing), and assert each names what `expected`
/// computes from them.
async fn assert_rendered(link: fn(&UserContexts) -> Option<&str>, expected: fn(&UserContexts) -> Value) {
    let tenant = PrivateTenant::provisioned().await;
    let contexts = UserContexts::start().await;
    let resources = RenderedResources::create(tenant.client(), &contexts.terms).await;

    let named = link(&contexts);
    let rendered = resources.rendered(tenant.client(), named).await;

    assert_every_resource_named(&rendered, &expected(&contexts));
    contexts.shutdown().await;
    tenant.finish().await;
}

/// Clauses 5.5.5 and C.7: with no request context, every
/// resource names the v1.9 core `@context` alone, as one string.
#[tokio::test]
async fn with_no_request_context_every_resource_names_the_core_context_as_a_string() {
    assert_rendered(|_| None, |_| core_value()).await;
}

/// Annex C.8: a context that ends with the v1.9 core `@context`
/// (the ETSI compound shape) is named alone, as one string.
#[tokio::test]
async fn a_context_ending_with_the_core_context_is_named_alone() {
    assert_rendered(|contexts| Some(contexts.wrapper.as_str()), |contexts| json!(contexts.wrapper)).await;
}

/// Clause 5.2.3: a user context without the core `@context` is
/// followed by it.
#[tokio::test]
async fn a_user_context_without_the_core_context_is_followed_by_it() {
    assert_rendered(|contexts| Some(contexts.terms.as_str()), |contexts| json!([contexts.terms, core_value()])).await;
}

/// Clause 4.4: a context that imports the core `@context` first
/// and then sets a `@vocab`, which JSON-LD protection does not cover, is
/// followed by the core `@context` again, so a standard processor reads the
/// core `@vocab` last as the broker does.
#[tokio::test]
async fn a_context_setting_a_vocabulary_after_the_core_context_is_followed_by_it() {
    assert_rendered(
        |contexts| Some(contexts.vocab_after_core.as_str()),
        |contexts| json!([contexts.vocab_after_core, core_value()]),
    )
    .await;
}

/// A context that imports the core `@context` first and then defines its own
/// terms leaves its own definitions in force last, and is followed by the core
/// `@context`.
#[tokio::test]
async fn a_context_defining_terms_after_importing_the_core_context_is_followed_by_it() {
    assert_rendered(
        |contexts| Some(contexts.importing.as_str()),
        |contexts| json!([contexts.importing, core_value()]),
    )
    .await;
}

/// Clause 4.4: a `Link` naming the v1.8 core `@context` alone is
/// answered under v1.9 alone.
#[tokio::test]
async fn an_older_core_release_alone_is_answered_as_the_current_one() {
    assert_rendered(|_| Some(CORE_V18), |_| core_value()).await;
}

/// The versionless alias alone is answered under v1.9 alone; the
/// alias is never emitted.
#[tokio::test]
async fn the_versionless_alias_alone_is_answered_as_the_current_core_context() {
    assert_rendered(|_| Some(CORE_VERSIONLESS), |_| core_value()).await;
}

/// A document importing v1.8 through `@import` beside its own terms
/// is processed with v1.9 and followed by it.
#[tokio::test]
async fn a_document_importing_an_older_core_release_is_followed_by_the_current_one() {
    assert_rendered(
        |contexts| Some(contexts.imports_v18.as_str()),
        |contexts| json!([contexts.imports_v18, core_value()]),
    )
    .await;
}

/// A document naming a wrapper that ends with the core `@context`
/// leaves it in force last through the chain, and is named alone.
#[tokio::test]
async fn a_chain_ending_with_the_core_context_is_named_alone() {
    assert_rendered(|contexts| Some(contexts.outer.as_str()), |contexts| json!(contexts.outer)).await;
}

/// A document ending with the older v1.8 core release is followed
/// by v1.9, which the broker processes last whatever release a document names.
#[tokio::test]
async fn a_document_ending_with_an_older_core_release_is_followed_by_the_current_one() {
    assert_rendered(|contexts| Some(contexts.v18_tail.as_str()), |contexts| json!([contexts.v18_tail, core_value()])).await;
}

/// A document whose last entry is a relative reference to a sibling
/// that ends with the core `@context` is resolved against its own URL and named
/// alone.
#[tokio::test]
async fn a_relative_reference_to_a_document_ending_with_the_core_context_is_resolved_and_named_alone() {
    assert_rendered(|contexts| Some(contexts.relative.as_str()), |contexts| json!(contexts.relative)).await;
}
