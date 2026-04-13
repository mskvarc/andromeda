#![allow(clippy::expect_used, clippy::unwrap_used)]
pub mod query_entities;
pub mod retrieve_entity;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    query_entities::tests(trials);
    retrieve_entity::tests(trials);
}
