pub mod query_entities;
pub mod retrieve_entity;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    retrieve_entity::tests(trials);
    query_entities::tests(trials);
}
