#![allow(clippy::expect_used, clippy::unwrap_used)]
pub mod entities;
pub mod entity_attributes;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    entities::tests(trials);
    entity_attributes::tests(trials);
}
