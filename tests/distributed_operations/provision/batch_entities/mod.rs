pub mod create;
pub mod delete;
pub mod upsert;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    create::tests(trials);
    upsert::tests(trials);
    delete::tests(trials);
}
