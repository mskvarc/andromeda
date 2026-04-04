pub mod attributes;
pub mod entity;
pub mod temporal;
pub mod types;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    entity::tests(trials);
    types::tests(trials);
    attributes::tests(trials);
    temporal::tests(trials);
}
