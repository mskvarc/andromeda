pub mod create;
pub mod delete;
pub mod merge;
pub mod update;
pub mod upsert;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    create::tests(trials);
    delete::tests(trials);
    merge::tests(trials);
    update::tests(trials);
    upsert::tests(trials);
}
