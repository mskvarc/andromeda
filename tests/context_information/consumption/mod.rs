#![allow(clippy::expect_used, clippy::unwrap_used)]
pub mod temporal_entity;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    temporal_entity::tests(trials);
}
