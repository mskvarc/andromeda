use crate::{
    BAD_REQUEST_DATA_TYPE,
    INVALID_REQUEST_TYPE,
    RESOURCE_NOT_FOUND_TYPE,
    assert_problem_detail_type,
    clean_database,
    client,
    create_csource_subscription,
    csource_subscription_payload,
    unique_subscription_id,
    update_csource_subscription,
    update_csource_subscription_raw,
    update_invalid_notification_fragment,
    update_invalid_type_fragment,
    update_notification_fragment,
};
use reqwest::StatusCode;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn csr_subscription_update_existing_returns_204() {
    clean_database().await;
    let http_client = client();
    let subscription_id = unique_subscription_id("update-existing");
    let payload = csource_subscription_payload(&subscription_id);

    let (create_status, _, _) = create_csource_subscription(&http_client, &payload).await;
    assert_eq!(create_status, StatusCode::CREATED);

    let fragment = update_notification_fragment();
    let (status, _) = update_csource_subscription(&http_client, &subscription_id, &fragment).await;

    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
#[serial]
async fn csr_subscription_update_invalid_uri_returns_400_bad_request_data() {
    clean_database().await;
    let http_client = client();
    let fragment = update_notification_fragment();

    let (status, body) = update_csource_subscription(&http_client, "invalidUri", &fragment).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
}

#[tokio::test]
#[serial]
async fn csr_subscription_update_unknown_id_returns_404_resource_not_found() {
    clean_database().await;
    let http_client = client();
    let fragment = update_notification_fragment();

    let (status, body) = update_csource_subscription(&http_client, "urn:ngsi-ld:Subscription:unknownSubscription", &fragment).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_problem_detail_type(&body, RESOURCE_NOT_FOUND_TYPE);
}

#[tokio::test]
#[serial]
async fn csr_subscription_update_invalid_type_fragment_returns_400_bad_request_data() {
    clean_database().await;
    let http_client = client();
    let subscription_id = unique_subscription_id("update-invalid-type-fragment");
    let payload = csource_subscription_payload(&subscription_id);

    let (create_status, _, _) = create_csource_subscription(&http_client, &payload).await;
    assert_eq!(create_status, StatusCode::CREATED);

    let fragment = update_invalid_type_fragment();
    let (status, body) = update_csource_subscription(&http_client, &subscription_id, &fragment).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
}

#[tokio::test]
#[serial]
async fn csr_subscription_update_invalid_notification_fragment_returns_400_bad_request_data() {
    clean_database().await;
    let http_client = client();
    let subscription_id = unique_subscription_id("update-invalid-notification-fragment");
    let payload = csource_subscription_payload(&subscription_id);

    let (create_status, _, _) = create_csource_subscription(&http_client, &payload).await;
    assert_eq!(create_status, StatusCode::CREATED);

    let fragment = update_invalid_notification_fragment();
    let (status, body) = update_csource_subscription(&http_client, &subscription_id, &fragment).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
}

#[tokio::test]
#[serial]
async fn csr_subscription_update_malformed_json_returns_400_invalid_request() {
    clean_database().await;
    let http_client = client();
    let subscription_id = unique_subscription_id("update-malformed-json");
    let payload = csource_subscription_payload(&subscription_id);

    let (create_status, _, _) = create_csource_subscription(&http_client, &payload).await;
    assert_eq!(create_status, StatusCode::CREATED);

    let malformed_json = br#"{
   \"notification\":{
      \"format\":\"keyValues\",,
      \"endpoint\":{
         \"uri\":\"http://my.second.endpoint.org/notify\",
         \"accept\":\"application/json\"
      }
   }
}"#;

    let (status, body) = update_csource_subscription_raw(&http_client, &subscription_id, malformed_json).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, INVALID_REQUEST_TYPE);
}
