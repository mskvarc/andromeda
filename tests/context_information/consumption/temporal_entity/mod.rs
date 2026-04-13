#![allow(clippy::expect_used, clippy::unwrap_used)]
pub mod query_temporal_evolution;
pub mod retrieve_temporal_evolution;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    query_temporal_evolution::tests(trials);
    retrieve_temporal_evolution::tests(trials);
}
