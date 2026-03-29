pub mod attribute_deleted;
pub mod basic_notifications;
pub mod dataset_id;
pub mod entity_created;
pub mod entity_deleted;
pub mod format_and_context;
pub mod join_notifications;
pub mod mqtt_notifications;
pub mod notification_structure;
pub mod show_changes;
pub mod subscription_status;
pub mod sys_attrs;
pub mod throttling;
pub mod type_selection;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    basic_notifications::tests(trials);
    entity_created::tests(trials);
    notification_structure::tests(trials);
    subscription_status::tests(trials);
    format_and_context::tests(trials);
    throttling::tests(trials);
    type_selection::tests(trials);
    sys_attrs::tests(trials);
    dataset_id::tests(trials);
    entity_deleted::tests(trials);
    attribute_deleted::tests(trials);
    join_notifications::tests(trials);
    show_changes::tests(trials);
    mqtt_notifications::tests(trials);
}

pub(crate) fn set_endpoint_uri(sub: &mut serde_json::Value, uri: &str) {
    sub["notification"]["endpoint"]["uri"] = serde_json::Value::String(uri.to_string());
}
