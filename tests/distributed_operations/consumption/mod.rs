#![allow(clippy::expect_used, clippy::unwrap_used)]
pub mod entity;
pub mod temporal;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    entity::tests(trials);
    temporal::tests(trials);
}
