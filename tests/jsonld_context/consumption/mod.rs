pub mod list;
pub mod serve;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    list::tests(trials);
    serve::tests(trials);
}
