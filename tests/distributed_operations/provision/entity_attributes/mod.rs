pub mod append_entity_attrs;
pub mod delete_entity_attrs;
pub mod partial_update_attr;
pub mod update_entity_attrs;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    append_entity_attrs::tests(trials);
    update_entity_attrs::tests(trials);
    partial_update_attr::tests(trials);
    delete_entity_attrs::tests(trials);
}
