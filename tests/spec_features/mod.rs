pub mod attribute_expiration;
pub mod contained_by;
pub mod entity_ordering;
pub mod extension_headers;
pub mod geometry_property_collection;
pub mod list_property_relationship;
pub mod notification_join_level;
pub mod prefer_header;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    notification_join_level::tests(trials);
    geometry_property_collection::tests(trials);
    extension_headers::tests(trials);
    contained_by::tests(trials);
    list_property_relationship::tests(trials);
    entity_ordering::tests(trials);
    prefer_header::tests(trials);
    attribute_expiration::tests(trials);
}
