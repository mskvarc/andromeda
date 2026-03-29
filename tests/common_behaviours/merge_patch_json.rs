use crate::harness::{client::RequestHeaders, fixtures, trial::trial};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

const PREFIX: &str = "common::merge_patch_json";

pub fn tests(trials: &mut Vec<Trial>) {
    // 044_01_01: Partial update entity attribute with application/merge-patch+json -> 204
    trials.push(trial(&format!("{PREFIX}::044_01_01::partial_update_attr_with_merge_patch"), |ctx| async move {
        let id = ctx.entity_id("urn:ngsi-ld:Vehicle:044_01");
        let entity = fixtures::load_fixture("entities/vehicle-simple-attributes.jsonld", &id);

        let resp = ctx.client.create_entity(&entity).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let fragment = fixtures::load_fragment("entities/fragment_entities/vehicle-brandname-fragment.json");

        let resp = ctx
            .client
            .request(
                Method::PATCH,
                &format!("/entities/{id}/attrs/brandName"),
                Some(&fragment),
                RequestHeaders::new().content_type("application/merge-patch+json"),
            )
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);
    }));

    // 044_02_01: Update subscription with application/merge-patch+json -> 204
    trials.push(trial(&format!("{PREFIX}::044_02_01::update_subscription_with_merge_patch"), |ctx| async move {
        let sub_id = format!("urn:ngsi-ld:Subscription:044_02:{}", uuid::Uuid::new_v4());
        ctx.track_subscription(&sub_id);
        let sub = fixtures::load_fixture("subscriptions/subscription.jsonld", &sub_id);

        let resp = ctx.client.create_subscription(&sub).await;
        assert_eq!(resp.status, StatusCode::CREATED);

        let update = fixtures::load_fragment("subscriptions/fragments/subscription-update.json");

        let resp = ctx
            .client
            .request(
                Method::PATCH,
                &format!("/subscriptions/{sub_id}"),
                Some(&update),
                RequestHeaders::new().content_type("application/merge-patch+json"),
            )
            .await;
        assert_eq!(resp.status, StatusCode::NO_CONTENT);
    }));
}
