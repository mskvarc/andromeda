use reqwest::StatusCode;

use crate::helpers::{
    client,
    create_subscription,
    default_subscription,
    inactive_subscription,
    query_subscriptions,
    unique_subscription_id,
    watched_attributes_subscription,
};

async fn create_three_subscriptions(http_client: &reqwest::Client, suffix: &str) -> [String; 3] {
    let id1 = unique_subscription_id(&format!("query-a-{suffix}"));
    let id2 = unique_subscription_id(&format!("query-b-{suffix}"));
    let id3 = unique_subscription_id(&format!("query-c-{suffix}"));

    let payload1 = default_subscription(&id1);
    let payload2 = watched_attributes_subscription(&id2);
    let payload3 = inactive_subscription(&id3);

    let (status, _, _) = create_subscription(http_client, &payload1).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _, _) = create_subscription(http_client, &payload2).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _, _) = create_subscription(http_client, &payload3).await;
    assert_eq!(status, StatusCode::CREATED);

    [id1, id2, id3]
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_query_returns_created_subscriptions_in_result_set() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let ids = create_three_subscriptions(&http_client, "all").await;

    let (status, _, body) = query_subscriptions(&http_client, "", true).await;

    assert_eq!(status, StatusCode::OK);
    let entries = body.as_array().expect("query result should be array");
    let returned_ids = entries.iter().filter_map(|entry| entry["id"].as_str()).collect::<Vec<_>>();

    assert!(returned_ids.contains(&ids[0].as_str()));
    assert!(returned_ids.contains(&ids[1].as_str()));
    assert!(returned_ids.contains(&ids[2].as_str()));
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_query_with_limit_one_offset_one_returns_single_item_and_prev_next_links() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let _ = create_three_subscriptions(&http_client, "paginate-mid").await;

    let (status, headers, body) = query_subscriptions(&http_client, "limit=1&offset=1", false).await;

    assert_eq!(status, StatusCode::OK);
    let entries = body.as_array().expect("query result should be array");
    assert_eq!(entries.len(), 1);

    let link = headers.get("link").and_then(|value| value.to_str().ok()).unwrap_or_default();
    assert!(link.contains("offset=0"));
    assert!(link.contains("rel=\"prev\""));
    assert!(link.contains("rel=\"next\""));
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_query_with_limit_one_offset_two_returns_single_item_and_prev_link_only() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let _ = create_three_subscriptions(&http_client, "paginate-last").await;

    let (status, headers, body) = query_subscriptions(&http_client, "limit=1&offset=2", false).await;

    assert_eq!(status, StatusCode::OK);
    let entries = body.as_array().expect("query result should be array");
    assert_eq!(entries.len(), 1);

    let link = headers.get("link").and_then(|value| value.to_str().ok()).unwrap_or_default();
    assert!(link.contains("offset=1"));
    assert!(link.contains("rel=\"prev\""));
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_query_with_high_limit_returns_created_subscriptions() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let ids = create_three_subscriptions(&http_client, "paginate-all").await;

    for _ in 0..6 {
        let (status, _, body) = query_subscriptions(&http_client, "limit=15&offset=0", false).await;

        assert_eq!(status, StatusCode::OK);
        let entries = body.as_array().expect("query result should be array");
        let returned_ids = entries.iter().filter_map(|entry| entry["id"].as_str()).collect::<Vec<_>>();

        if returned_ids.contains(&ids[0].as_str()) && returned_ids.contains(&ids[1].as_str()) && returned_ids.contains(&ids[2].as_str()) {
            return;
        }

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    }

    panic!("query result did not include all created subscriptions within the retry window");
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_query_with_invalid_parameter_returns_bad_request() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;

    let (status, _, body) = query_subscriptions(&http_client, "invalidParam=invalidValue", false).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["type"], "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest");
}
