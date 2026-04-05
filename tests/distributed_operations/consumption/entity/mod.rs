pub mod query_entities;
pub mod retrieve_entity;
pub mod via_loop_detection;
pub mod warning_headers;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    retrieve_entity::tests(trials);
    query_entities::tests(trials);
    warning_headers::tests(trials);
    via_loop_detection::tests(trials);
}
