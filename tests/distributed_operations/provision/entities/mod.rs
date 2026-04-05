pub mod create_entity;
pub mod delete_entity;
pub mod merge_entity;
pub mod replace_entity;

use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    create_entity::tests(trials);
    delete_entity::tests(trials);
    replace_entity::tests(trials);
    merge_entity::tests(trials);
}

/// Backwards-compatible wrapper around the shared prepare_registration.
pub(crate) fn prepare_registration(fixture_path: &str, reg_id: &str, entity_id: &str, endpoint: &str, mode: Option<&str>) -> serde_json::Value {
    crate::distributed_operations::prepare_registration(fixture_path, reg_id, entity_id, endpoint, mode, None, None)
}
