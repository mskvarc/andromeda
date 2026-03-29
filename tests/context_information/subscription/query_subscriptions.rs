use crate::harness::{
    client::{QueryParams, RequestHeaders},
    fixtures,
    trial::trial,
};
use libtest_mimic::Trial;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

const PREFIX: &str = "subscription::query";

fn strip_ignored_keys(val: &mut serde_json::Value, keys: &[&str]) {
    match val {
        serde_json::Value::Object(map) => {
            for key in keys {
                map.remove(*key);
            }
            for v in map.values_mut() {
                strip_ignored_keys(v, keys);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                strip_ignored_keys(v, keys);
            }
        }
        _ => {}
    }
}

/// Create the 3 standard test subscriptions and return their IDs in order.
async fn create_three_subscriptions(ctx: &crate::harness::TestCtx) -> [String; 3] {
    let id1 = format!("urn:ngsi-ld:Subscription:query_a:{}", uuid::Uuid::new_v4());
    let id2 = format!("urn:ngsi-ld:Subscription:query_b:{}", uuid::Uuid::new_v4());
    let id3 = format!("urn:ngsi-ld:Subscription:query_c:{}", uuid::Uuid::new_v4());

    ctx.track_subscription(&id1);
    ctx.track_subscription(&id2);
    ctx.track_subscription(&id3);

    let sub1 = fixtures::load_fixture("subscriptions/subscription.jsonld", &id1);
    let sub2 = fixtures::load_fixture("subscriptions/subscription-watchedAttributes.jsonld", &id2);
    let sub3 = fixtures::load_fixture("subscriptions/subscription-inactive.jsonld", &id3);

    let resp = ctx.client.create_subscription(&sub1).await;
    assert_eq!(resp.status, StatusCode::CREATED);
    let resp = ctx.client.create_subscription(&sub2).await;
    assert_eq!(resp.status, StatusCode::CREATED);
    let resp = ctx.client.create_subscription(&sub3).await;
    assert_eq!(resp.status, StatusCode::CREATED);

    [id1, id2, id3]
}

pub fn tests(trials: &mut Vec<Trial>) {
    // ── 031_01: Query subscriptions ──────────────────────────────────

    // 031_01_01: Query all subscriptions and verify content (5.8.4)
    trials.push(trial(&format!("{PREFIX}::031_01_01::query_subscriptions"), |ctx| async move {
        let ids = create_three_subscriptions(&ctx).await;

        let resp = ctx
            .client
            .query_subscriptions(
                &QueryParams::new(),
                RequestHeaders::new().accept_ld_json().link_context(fixtures::test_suite_context_url()),
            )
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let actual: serde_json::Value = resp.json();
        let arr = actual.as_array().expect("response should be an array");
        assert_eq!(arr.len(), 3, "should return 3 subscriptions");

        let mut expected = fixtures::load_expectation("subscriptions/expectations/subscriptions-031-01.json");
        // Set expected IDs to match our generated ones
        expected[0]["id"] = serde_json::Value::String(ids[0].clone());
        expected[1]["id"] = serde_json::Value::String(ids[1].clone());
        expected[2]["id"] = serde_json::Value::String(ids[2].clone());

        let ignored = &["jsonldContext", "timesFailed", "timesSent", "notificationTrigger", "@context"];

        // Sort both arrays by ID for stable comparison
        let mut actual_arr: Vec<serde_json::Value> = arr.clone();
        actual_arr.sort_by(|a, b| a["id"].as_str().unwrap_or("").cmp(b["id"].as_str().unwrap_or("")));
        let mut expected_arr: Vec<serde_json::Value> = expected.as_array().unwrap().clone();
        expected_arr.sort_by(|a, b| a["id"].as_str().unwrap_or("").cmp(b["id"].as_str().unwrap_or("")));

        for v in &mut actual_arr {
            strip_ignored_keys(v, ignored);
        }
        for v in &mut expected_arr {
            strip_ignored_keys(v, ignored);
        }
        assert_eq!(expected_arr, actual_arr);
    }));

    // ── 031_02: Query subscriptions with pagination ──────────────────

    // 031_02_01: Query second subscription (limit=1, offset=1) (5.8.4)
    trials.push(trial(&format!("{PREFIX}::031_02_01::pagination_second"), |ctx| async move {
        let _ids = create_three_subscriptions(&ctx).await;

        let resp = ctx
            .client
            .query_subscriptions(&QueryParams::new().limit(1).offset(1), RequestHeaders::new().accept_ld_json())
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("response should be an array");
        assert_eq!(arr.len(), 1, "should return 1 subscription");

        let link = resp.headers.get("link").map(|v| v.to_str().unwrap_or("").to_string()).unwrap_or_default();
        assert!(link.contains("offset=0"), "should have prev link with offset=0");
        assert!(link.contains("offset=2"), "should have next link with offset=2");
        assert!(link.contains("rel=\"prev\""), "should have prev rel");
        assert!(link.contains("rel=\"next\""), "should have next rel");
    }));

    // 031_02_02: Query last subscription (limit=1, offset=2) (5.8.4)
    trials.push(trial(&format!("{PREFIX}::031_02_02::pagination_last"), |ctx| async move {
        let _ids = create_three_subscriptions(&ctx).await;

        let resp = ctx
            .client
            .query_subscriptions(&QueryParams::new().limit(1).offset(2), RequestHeaders::new().accept_ld_json())
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("response should be an array");
        assert_eq!(arr.len(), 1, "should return 1 subscription");

        let link = resp.headers.get("link").map(|v| v.to_str().unwrap_or("").to_string()).unwrap_or_default();
        assert!(link.contains("offset=1"), "should have prev link with offset=1");
        assert!(link.contains("rel=\"prev\""), "should have prev rel");
        assert!(!link.contains("rel=\"next\""), "should NOT have next link");
    }));

    // 031_02_03: Query all subscriptions (limit=15, offset=0) (5.8.4)
    trials.push(trial(&format!("{PREFIX}::031_02_03::pagination_all"), |ctx| async move {
        let _ids = create_three_subscriptions(&ctx).await;

        let resp = ctx
            .client
            .query_subscriptions(&QueryParams::new().limit(15).offset(0), RequestHeaders::new().accept_ld_json())
            .await;
        assert_eq!(resp.status, StatusCode::OK);

        let body: serde_json::Value = resp.json();
        let arr = body.as_array().expect("response should be an array");
        assert_eq!(arr.len(), 3, "should return 3 subscriptions");

        let link = resp.headers.get("link").map(|v| v.to_str().unwrap_or("").to_string()).unwrap_or_default();
        assert!(!link.contains("rel=\"prev\""), "should NOT have prev link");
        assert!(!link.contains("rel=\"next\""), "should NOT have next link");
    }));
}
