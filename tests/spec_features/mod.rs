pub mod attribute_expiration;
pub mod contained_by;
pub mod entity_ordering;
pub mod geometry_property_collection;
pub mod lang;
pub mod list_property_relationship;
pub mod notification_join_level;
pub mod prefer_header;
pub mod snapshot_api;
pub mod snapshot_notifications;
pub mod snapshot_scoping;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    notification_join_level::tests(trials);
    geometry_property_collection::tests(trials);
    contained_by::tests(trials);
    list_property_relationship::tests(trials);
    snapshot_api::tests(trials);
    snapshot_notifications::tests(trials);
    snapshot_scoping::tests(trials);
    lang::tests(trials);
    entity_ordering::tests(trials);
    prefer_header::tests(trials);
    attribute_expiration::tests(trials);
}
