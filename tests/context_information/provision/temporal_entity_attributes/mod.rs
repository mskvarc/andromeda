#![allow(clippy::expect_used, clippy::unwrap_used)]
pub mod add;
pub mod delete;
pub mod delete_instance;
pub mod partial_update;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    add::tests(trials);
    delete::tests(trials);
    delete_instance::tests(trials);
    partial_update::tests(trials);
}
