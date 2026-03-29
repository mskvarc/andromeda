use crate::harness::{fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

const PREFIX: &str = "common::ld_context_not_available";
const ERROR_TYPE: &str = "https://uri.etsi.org/ngsi-ld/errors/LdContextNotAvailable";

pub fn tests(trials: &mut Vec<Trial>) {
    // 043_01_01: Create entity with unretrievable @context -> 503
    trials.push(trial(&format!("{PREFIX}::043_01_01::create_entity_unretrievable_context"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Building:043_01");
        let entity = fixtures::load_fixture("entities/building-unretrievable-context.jsonld", &id);

        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::SERVICE_UNAVAILABLE);
        let body = resp.json_value();
        assert_eq!(body["type"], ERROR_TYPE);
    }));

    // 043_01_02: Create subscription with unretrievable @context -> 503
    trials.push(trial(
        &format!("{PREFIX}::043_01_02::create_subscription_unretrievable_context"),
        |ctx| async move {
            let sub_id = format!("urn:ngsi-ld:Subscription:043_02:{}", uuid::Uuid::new_v4());
            let sub = fixtures::load_fixture("subscriptions/subscription-unretrievable-context.jsonld", &sub_id);

            let resp = ctx.client.create_subscription(&sub).await;
            assert_eq!(resp.status, StatusCode::SERVICE_UNAVAILABLE);
            let body = resp.json_value();
            assert_eq!(body["type"], ERROR_TYPE);
        },
    ));

    // 043_01_03: Create temporal entity with unretrievable @context -> 503
    trials.push(trial(
        &format!("{PREFIX}::043_01_03::create_temporal_entity_unretrievable_context"),
        |ctx| async move {
            let id = ctx.entity_id("urn:ngsi-ld:Bus:043_03");
            let entity = fixtures::load_fixture("temporal_entities/bus-temporal-representation-unretrievable-context.jsonld", &id);

            let resp = ctx.client.create_temporal_entity(&entity).await;
            assert_eq!(resp.status, StatusCode::SERVICE_UNAVAILABLE);
            let body = resp.json_value();
            assert_eq!(body["type"], ERROR_TYPE);
        },
    ));

    // 043_01_04: Batch create entities with unretrievable @context -> 207 with errors
    trials.push(trial(&format!("{PREFIX}::043_01_04::batch_create_unretrievable_context"), |ctx| async move {
        let id1 = ctx.entity_id("urn:ngsi-ld:Building:043_04a");
        let id2 = ctx.entity_id("urn:ngsi-ld:Building:043_04b");
        let e1 = fixtures::load_fixture("entities/building-unretrievable-context.jsonld", &id1);
        let e2 = fixtures::load_fixture("entities/building-unretrievable-context.jsonld", &id2);
        let entities = serde_json::json!([e1, e2]);

        let resp = ctx.client.batch_create(&entities).await;
        assert!(
            resp.status == StatusCode::MULTI_STATUS || resp.status == StatusCode::SERVICE_UNAVAILABLE,
            "expected 207 or 503, got {}",
            resp.status
        );
    }));

    // 043_01_05: Create CSR with unretrievable @context -> 503
    trials.push(trial(
        &format!("{PREFIX}::043_01_05::create_registration_unretrievable_context"),
        |ctx| async move {
            let id = format!("urn:ngsi-ld:ContextSourceRegistration:043_05:{}", uuid::Uuid::new_v4());
            let reg = fixtures::load_fixture("csource_registrations/context-source-registration-unretrievable-context.jsonld", &id);

            let resp = ctx.client.create_registration(&reg).await;
            assert_eq!(resp.status, StatusCode::SERVICE_UNAVAILABLE);
            let body = resp.json_value();
            assert_eq!(body["type"], ERROR_TYPE);
        },
    ));
}
