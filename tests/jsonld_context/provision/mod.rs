pub mod add;
pub mod delete;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    add::tests(trials);
    delete::tests(trials);
}
