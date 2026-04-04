pub mod query_types;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    query_types::tests(trials);
}
