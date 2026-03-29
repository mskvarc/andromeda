pub mod append;
pub mod delete;
pub mod partial_update;
pub mod replace;
pub mod update;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    append::tests(trials);
    update::tests(trials);
    delete::tests(trials);
    partial_update::tests(trials);
    replace::tests(trials);
}
