pub mod create_entity;
pub mod delete_entities_by_query;
pub mod delete_entity;
pub mod merge_entity;
pub mod replace_entity;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    create_entity::tests(trials);
    delete_entity::tests(trials);
    delete_entities_by_query::tests(trials);
    merge_entity::tests(trials);
    replace_entity::tests(trials);
}
