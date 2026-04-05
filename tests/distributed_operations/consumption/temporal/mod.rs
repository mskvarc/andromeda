pub mod entity_map_temporal;
pub mod query_temporal;
pub mod retrieve_temporal;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    entity_map_temporal::tests(trials);
    query_temporal::tests(trials);
    retrieve_temporal::tests(trials);
}
