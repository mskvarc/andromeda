#![allow(clippy::expect_used, clippy::unwrap_used)]
pub mod temporal_entity;
pub mod temporal_entity_attributes;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    temporal_entity::tests(trials);
    temporal_entity_attributes::tests(trials);
}
