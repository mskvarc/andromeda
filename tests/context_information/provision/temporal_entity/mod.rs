pub mod create;
pub mod delete;
pub mod update;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    create::tests(trials);
    delete::tests(trials);
    update::tests(trials);
}
