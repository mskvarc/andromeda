pub mod query_temporal;
pub mod retrieve_temporal;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    query_temporal::tests(trials);
    retrieve_temporal::tests(trials);
}
