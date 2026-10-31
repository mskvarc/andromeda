//! List contexts answers every implicitly created context beyond a thousand.

use andromeda_test_suite::{
    broker,
    broker::BrokerClient,
    config::SuiteConfig,
    context_listing_walk::{ListingWalk, entry_count},
    link_header::link_text,
    minted_wrapper::minted_at,
    namespace::Namespace,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType},
    subscription::{Endpoint, NotificationParams, Subscription, SubscriptionId},
};

/// Pins: List @contexts of kind `ImplicitlyCreated`, with and without details and with no kind filter, walked by
/// its next pointer from a page of 1000, answers every `ImplicitlyCreated` @context stored, the first and the last
/// of more than a thousand each on exactly one page
/// Clause: 5.13.3.3, 5.13.3.4, 5.13.1, 6.29.3.2
/// Tenant: private (the subject is the completeness of the `ImplicitlyCreated` listing, which only a tenant holding
/// exactly the wrappers this file causes can show once more than 1000 exist)
#[gtest]
#[tokio::test]
async fn n_ctx_lst_3002_list_contexts_answers_every_implicitly_created_context_beyond_a_thousand() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let first = subscribe(&tenant, &namespace, "first").await?;
    for index in 0..1000 {
        subscribe(&tenant, &namespace, &format!("many-{index}")).await?;
    }
    let last = subscribe(&tenant, &namespace, "last").await?;
    let (first_id, last_id) = (wrapper_id(&tenant, &first).await?, wrapper_id(&tenant, &last).await?);
    // Clause 5.13.3.4 has the broker provide an entry for each @context currently stored, and clause 5.5.9.1 pages
    // the listing, so the walk over its two pages holds the first and the last each on exactly one page.
    for (query, ends_listing) in [
        ("limit=1000&kind=ImplicitlyCreated&details=true", true),
        ("limit=1000&kind=ImplicitlyCreated", true),
        ("limit=1000&details=true", false),
    ] {
        let walk = ListingWalk::read(&tenant, query).await?;
        expect_that!(entry_count(walk.first())?, eq(1000));
        expect_that!(entry_count(walk.second())?, ge(2));
        walk.require_on_one_page(&first_id)?;
        walk.require_on_one_page(&last_id)?;
        if ends_listing {
            expect_that!(link_text(walk.second().headers()), not(contains_substring("rel=\"next\"")));
        }
    }
    Ok(())
}

/// Creates a subscription named after `name` with an inline @context defining the term `name`.
async fn subscribe(tenant: &BrokerClient, namespace: &Namespace, name: &str) -> Result<SubscriptionId> {
    let unreachable = SuiteConfig::get()?.unreachable_origin().origin().ascii_serialization();
    let id = namespace.subscription_id(name)?;
    let subscription = Subscription::builder()
        .id(id.clone())
        .entities([namespace.entity_type("Building")?.to_string()])
        .notification(
            NotificationParams::builder(
                Endpoint::builder(format!("{unreachable}/notify/{namespace}/{name}"))
                    .accept(NgsiLdMediaType::Json)
                    .build()?,
            )
            .build()?,
        )
        .build()?;
    tenant
        .with_context(Context::Single(ContextEntry::Inline(
            format!(r#"{{"{name}": "https://test.andromeda/{namespace}/{name}"}}"#).parse::<InlineContext>()?,
        )))?
        .with_context_delivery(ContextDelivery::LdJsonBody)
        .subscriptions()
        .create(&subscription)
        .send()
        .await?;
    Ok(id)
}

/// The identifier of the wrapper hosted for the subscription `id`, read from its `jsonldContext`.
async fn wrapper_id(tenant: &BrokerClient, id: &SubscriptionId) -> Result<String> {
    let retrieved = tenant
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(id)
        .send()
        .await?;
    Ok(minted_at(retrieved.jsonld_context().or_fail()?)?.id.to_string())
}
