mod create;
mod delete;
mod update;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    create::tests(trials);
    update::tests(trials);
    delete::tests(trials);
}
