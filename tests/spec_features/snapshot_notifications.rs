/// Feature 8: Snapshot Status Notifications (spec 5.16.6, 5.3.4)
///
/// Verifies that the broker sends `SnapshotNotification` messages to a
/// snapshot's configured endpoint on create, update, and clone operations.
use crate::harness::{notification_receiver::NotificationReceiver, trial::trial};
use libtest_mimic::Trial;
use reqwest::StatusCode;
use std::time::Duration;

const PREFIX: &str = "spec_features::snapshot_notifications";

pub fn tests(trials: &mut Vec<Trial>) {
    // -- Notification sent on create with endpoint configured ------
    trials.push(trial(&format!("{PREFIX}::notification_sent_on_create"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let body = serde_json::json!({
            "snapshotLifetime": "PT1H",
            "snapshotPriority": 7,
            "endpoint": receiver.endpoint_url()
        });
        let resp = ctx.client.create_snapshot(&body).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let created: serde_json::Value = resp.json();
        let snap_id = created["id"].as_str().unwrap().to_string();
        ctx.track_snapshot(&snap_id);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["type"], "SnapshotNotification");
        assert_eq!(notif.body["snapshotId"], snap_id);
        assert_eq!(notif.body["snapshotStatus"], "preparing");
        assert_eq!(notif.body["snapshotPriority"], 7);
        assert!(notif.body["expiresAt"].is_string(), "expiresAt should be present");
        assert!(notif.body["notifiedAt"].is_string(), "notifiedAt should be present");
        assert!(
            notif.body["id"].as_str().unwrap().starts_with("urn:ngsi-ld:SnapshotNotification:"),
            "notification id should follow URN pattern"
        );
    }));

    // -- No notification when endpoint is not configured -----------
    trials.push(trial(&format!("{PREFIX}::no_notification_without_endpoint"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let body = serde_json::json!({
            "snapshotLifetime": "PT1H",
            "snapshotPriority": 5
        });
        let resp = ctx.client.create_snapshot(&body).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let created: serde_json::Value = resp.json();
        let snap_id = created["id"].as_str().unwrap().to_string();
        ctx.track_snapshot(&snap_id);

        // Update it too, to be sure
        let patch = serde_json::json!({"snapshotPriority": 8});
        let resp = ctx.client.update_snapshot(&snap_id, &patch).await;
        assert_eq!(resp.status, StatusCode::OK);

        receiver.wait_for_no_notification(Duration::from_secs(2)).await;
    }));

    // -- Notification sent on update with correct fields -----------
    trials.push(trial(&format!("{PREFIX}::notification_sent_on_update_with_correct_fields"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let body = serde_json::json!({
            "snapshotLifetime": "PT1H",
            "snapshotPriority": 3,
            "endpoint": receiver.endpoint_url()
        });
        let resp = ctx.client.create_snapshot(&body).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let created: serde_json::Value = resp.json();
        let snap_id = created["id"].as_str().unwrap().to_string();
        ctx.track_snapshot(&snap_id);

        // Drain the create notification
        let _create_notif = receiver.wait_for_notification(Duration::from_secs(5)).await;

        // Update priority and lifetime
        let patch = serde_json::json!({
            "snapshotLifetime": "PT2H",
            "snapshotPriority": 8
        });
        let resp = ctx.client.update_snapshot(&snap_id, &patch).await;
        assert_eq!(resp.status, StatusCode::OK);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["type"], "SnapshotNotification");
        assert_eq!(notif.body["snapshotId"], snap_id);
        assert_eq!(notif.body["snapshotPriority"], 8, "should reflect updated priority");
        assert!(notif.body["expiresAt"].is_string(), "expiresAt should be present");
        assert!(notif.body["notifiedAt"].is_string(), "notifiedAt should be present");
    }));

    // -- receiverInfo headers included in notification -------------
    trials.push(trial(&format!("{PREFIX}::receiver_info_headers_included_in_notification"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let body = serde_json::json!({
            "snapshotLifetime": "PT1H",
            "snapshotPriority": 5,
            "endpoint": receiver.endpoint_url(),
            "receiverInfo": [
                {"key": "X-Custom-Header", "value": "test-value-123"}
            ]
        });
        let resp = ctx.client.create_snapshot(&body).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let created: serde_json::Value = resp.json();
        ctx.track_snapshot(created["id"].as_str().unwrap());

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["type"], "SnapshotNotification");

        let custom_header = notif.headers.get("X-Custom-Header").expect("should have custom header");
        assert_eq!(custom_header.to_str().unwrap(), "test-value-123");
    }));

    // -- Notification sent on clone with inherited endpoint --------
    trials.push(trial(&format!("{PREFIX}::notification_on_clone_inherits_endpoint"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        let body = serde_json::json!({
            "snapshotLifetime": "PT1H",
            "snapshotPriority": 6,
            "endpoint": receiver.endpoint_url()
        });
        let resp = ctx.client.create_snapshot(&body).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let created: serde_json::Value = resp.json();
        let source_id = created["id"].as_str().unwrap().to_string();
        ctx.track_snapshot(&source_id);

        // Drain the create notification
        let _create_notif = receiver.wait_for_notification(Duration::from_secs(5)).await;

        // Clone the snapshot
        let resp = ctx.client.clone_snapshot(&source_id).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let cloned: serde_json::Value = resp.json();
        let cloned_id = cloned["id"].as_str().unwrap().to_string();
        ctx.track_snapshot(&cloned_id);

        let notif = receiver.wait_for_notification(Duration::from_secs(5)).await;
        assert_eq!(notif.body["type"], "SnapshotNotification");
        assert_eq!(notif.body["snapshotId"], cloned_id, "should reference the cloned snapshot");
        assert_ne!(
            notif.body["snapshotId"].as_str().unwrap(),
            source_id,
            "should not reference the source snapshot"
        );
        assert_eq!(notif.body["snapshotPriority"], 6, "should inherit source priority");
    }));

    // -- No notification when status is preparing and no endpoint --
    trials.push(trial(&format!("{PREFIX}::no_notification_when_no_endpoint_on_clone"), |ctx| async move {
        let receiver = NotificationReceiver::start().await;

        // Create source WITHOUT endpoint
        let body = serde_json::json!({
            "snapshotLifetime": "PT1H",
            "snapshotPriority": 4
        });
        let resp = ctx.client.create_snapshot(&body).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let created: serde_json::Value = resp.json();
        let source_id = created["id"].as_str().unwrap().to_string();
        ctx.track_snapshot(&source_id);

        // Clone it
        let resp = ctx.client.clone_snapshot(&source_id).await;
        assert_eq!(resp.status, StatusCode::CREATED);
        let cloned: serde_json::Value = resp.json();
        ctx.track_snapshot(cloned["id"].as_str().unwrap());

        receiver.wait_for_no_notification(Duration::from_secs(2)).await;
    }));
}
