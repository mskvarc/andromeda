//! Which context sources a local subscription's context source filter places
//! its reduced copies at (Table 5.2.12-1, clauses 5.8.1.4 and 5.11.2.4), run
//! against a service wired to in-memory doubles.

use crate::reassembly_harness::{
    ReassemblyScene,
    SceneSpec,
    identifier,
    subscribing_source::{SPEED, SubscribingSource},
};
use serde_json::json;

/// The `csf` of the local subscription as the store holds it, its property
/// term expanded: the sources whose registration has the `tier` "gold".
const GOLD_TIER: &str = r#""https://uri.etsi.org/ngsi-ld/default-context/tier"=="gold""#;

/// [`GOLD_TIER`] as the copy carries it: the core context a copy without a `jsonldContext` is written under abbreviates `tier`.
const GOLD_TIER_FORWARDED: &str = r#"tier=="gold""#;

/// Table 5.2.12-1 has the `csf` of an entity subscription "met by Context
/// Source Registrations describing Context Sources to be used", so the copy
/// goes to the source registered in the gold tier alone, carrying the `csf`
/// spelled in the `@context` the copy is written under, and to neither the silver one nor the one
/// registered without a tier.
#[tokio::test]
async fn a_context_source_filter_places_the_copy_only_at_the_sources_whose_registration_meets_it() {
    let gold = SubscribingSource::start(&identifier("urn:ngsi-ld:ContextSourceRegistration:gold")).await;
    let silver = SubscribingSource::start(&identifier("urn:ngsi-ld:ContextSourceRegistration:silver")).await;
    let untiered = SubscribingSource::start(&identifier("urn:ngsi-ld:ContextSourceRegistration:untiered")).await;
    let scene = ReassemblyScene::start(SceneSpec {
        subscription_members: json!({"watchedAttributes": [SPEED], "csf": GOLD_TIER}),
        registrations: vec![gold.stored_in_tier("gold"), silver.stored_in_tier("silver"), untiered.stored()],
        ..SceneSpec::default()
    })
    .await;

    scene.create().await.expect("the subscription is created");

    let placed = gold.placed().await;
    let [copy] = placed.as_slice() else {
        panic!("one copy is placed at the gold source: {placed:?}");
    };
    pretty_assertions::assert_eq!(copy["csf"], json!(GOLD_TIER_FORWARDED));
    pretty_assertions::assert_eq!(silver.placed().await, Vec::<serde_json::Value>::new());
    pretty_assertions::assert_eq!(untiered.placed().await, Vec::<serde_json::Value>::new());
}

/// Without a `csf` the copy goes to every source the selectors and the
/// watched attributes match, whatever tier its registration declares.
#[tokio::test]
async fn a_subscription_without_a_context_source_filter_places_a_copy_at_every_matching_source() {
    let gold = SubscribingSource::start(&identifier("urn:ngsi-ld:ContextSourceRegistration:gold")).await;
    let silver = SubscribingSource::start(&identifier("urn:ngsi-ld:ContextSourceRegistration:silver")).await;
    let untiered = SubscribingSource::start(&identifier("urn:ngsi-ld:ContextSourceRegistration:untiered")).await;
    let scene = ReassemblyScene::start(SceneSpec {
        subscription_members: json!({"watchedAttributes": [SPEED]}),
        registrations: vec![gold.stored_in_tier("gold"), silver.stored_in_tier("silver"), untiered.stored()],
        ..SceneSpec::default()
    })
    .await;

    scene.create().await.expect("the subscription is created");

    for source in [&gold, &silver, &untiered] {
        let placed = source.placed().await;
        pretty_assertions::assert_eq!(placed.len(), 1, "{placed:?}");
        assert!(placed.first().is_some_and(|copy| copy.get("csf").is_none()), "{placed:?}");
    }
}
