use crate::{
    BAD_REQUEST_DATA_TYPE,
    assert_problem_detail_type,
    clean_database,
    client,
    create_csource_subscription,
    csource_subscription_payload,
    query_csource_subscriptions,
    raw_get,
    unique_subscription_id,
};
use reqwest::StatusCode;
use serial_test::serial;

async fn create_subscriptions(http_client: &reqwest::Client, count: usize) {
    for index in 0..count {
        let subscription_id = unique_subscription_id(&format!("query-{index}"));
        let payload = csource_subscription_payload(&subscription_id);
        let (status, _, _) = create_csource_subscription(http_client, &payload).await;
        assert_eq!(status, StatusCode::CREATED);
    }
}

#[tokio::test]
#[serial]
async fn csr_subscription_query_all_returns_at_least_created_subscriptions() {
    clean_database().await;
    let http_client = client();
    create_subscriptions(&http_client, 2).await;

    let (status, body) = query_csource_subscriptions(&http_client, "").await;

    assert_eq!(status, StatusCode::OK);
    let items = match body.as_array() {
        Some(items) => items,
        None => panic!("expected array response, got: {body}"),
    };
    assert!(items.len() >= 2, "expected at least 2 subscriptions, got {}", items.len());
}

#[tokio::test]
#[serial]
async fn csr_subscription_query_limit_1_returns_one_result() {
    clean_database().await;
    let http_client = client();
    create_subscriptions(&http_client, 3).await;

    let (status, body) = query_csource_subscriptions(&http_client, "limit=1").await;

    assert_eq!(status, StatusCode::OK);
    let items = match body.as_array() {
        Some(items) => items,
        None => panic!("expected array response, got: {body}"),
    };
    assert_eq!(items.len(), 1);
}

#[tokio::test]
#[serial]
async fn csr_subscription_query_limit_2_returns_two_results() {
    clean_database().await;
    let http_client = client();
    create_subscriptions(&http_client, 3).await;

    let (status, body) = query_csource_subscriptions(&http_client, "limit=2").await;

    assert_eq!(status, StatusCode::OK);
    let items = match body.as_array() {
        Some(items) => items,
        None => panic!("expected array response, got: {body}"),
    };
    assert_eq!(items.len(), 2);
}

#[tokio::test]
#[serial]
async fn csr_subscription_query_limit_above_total_returns_all_results() {
    clean_database().await;
    let http_client = client();
    create_subscriptions(&http_client, 3).await;

    let (status, body) = query_csource_subscriptions(&http_client, "limit=15").await;

    assert_eq!(status, StatusCode::OK);
    let items = match body.as_array() {
        Some(items) => items,
        None => panic!("expected array response, got: {body}"),
    };
    assert_eq!(items.len(), 3);
}

#[tokio::test]
#[serial]
async fn csr_subscription_query_limit_1_offset_1_returns_one_result() {
    clean_database().await;
    let http_client = client();
    create_subscriptions(&http_client, 3).await;

    let (status, body) = query_csource_subscriptions(&http_client, "limit=1&offset=1").await;

    assert_eq!(status, StatusCode::OK);
    let items = match body.as_array() {
        Some(items) => items,
        None => panic!("expected array response, got: {body}"),
    };
    assert_eq!(items.len(), 1);
}

#[tokio::test]
#[serial]
async fn csr_subscription_query_limit_2_offset_2_returns_one_result() {
    clean_database().await;
    let http_client = client();
    create_subscriptions(&http_client, 3).await;

    let (status, body) = query_csource_subscriptions(&http_client, "limit=2&offset=2").await;

    assert_eq!(status, StatusCode::OK);
    let items = match body.as_array() {
        Some(items) => items,
        None => panic!("expected array response, got: {body}"),
    };
    assert_eq!(items.len(), 1);
}

#[tokio::test]
#[serial]
async fn csr_subscription_query_limit_15_offset_0_returns_all_results() {
    clean_database().await;
    let http_client = client();
    create_subscriptions(&http_client, 3).await;

    let (status, body) = query_csource_subscriptions(&http_client, "limit=15&offset=0").await;

    assert_eq!(status, StatusCode::OK);
    let items = match body.as_array() {
        Some(items) => items,
        None => panic!("expected array response, got: {body}"),
    };
    assert_eq!(items.len(), 3);
}

#[tokio::test]
#[serial]
async fn csr_subscription_query_negative_limit_returns_400_bad_request_data() {
    clean_database().await;
    let http_client = client();

    let (status, body) = raw_get(&http_client, "/csourceSubscriptions?limit=-5&offset=2").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
}

#[tokio::test]
#[serial]
async fn csr_subscription_query_negative_offset_returns_400_bad_request_data() {
    clean_database().await;
    let http_client = client();

    let (status, body) = raw_get(&http_client, "/csourceSubscriptions?limit=2&offset=-3").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
}

#[tokio::test]
#[serial]
async fn csr_subscription_query_zero_limit_returns_400_bad_request_data() {
    clean_database().await;
    let http_client = client();

    let (status, body) = raw_get(&http_client, "/csourceSubscriptions?limit=0&offset=0").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
}

#[tokio::test]
#[serial]
async fn csr_subscription_query_non_numeric_limit_returns_400_bad_request_data() {
    clean_database().await;
    let http_client = client();

    let (status, body) = raw_get(&http_client, "/csourceSubscriptions?limit=abc&offset=0").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
}
