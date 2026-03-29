pub mod contained_by;
pub mod geometry_property_collection;
pub mod list_property_relationship;
pub mod notification_join_level;
pub mod snapshot_api;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    notification_join_level::tests(trials);
    geometry_property_collection::tests(trials);
    contained_by::tests(trials);
    list_property_relationship::tests(trials);
    snapshot_api::tests(trials);
}
