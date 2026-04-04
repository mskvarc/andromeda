pub mod query_attributes;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    query_attributes::tests(trials);
}
