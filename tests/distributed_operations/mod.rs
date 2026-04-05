pub mod consumption;
pub mod provision;

use crate::harness::fixtures;
use libtest_mimic::Trial;

pub fn tests(trials: &mut Vec<Trial>) {
    provision::tests(trials);
    consumption::tests(trials);
}

/// Load a CSR fixture and replace its id, endpoint, entity id within information, and optionally set mode and operations.
pub(crate) fn prepare_registration(
    fixture_path: &str,
    reg_id: &str,
    entity_id: &str,
    endpoint: &str,
    mode: Option<&str>,
    operations: Option<&[&str]>,
    context_source_alias: Option<&str>,
) -> serde_json::Value {
    let mut reg = fixtures::load_fixture(fixture_path, reg_id);
    let obj = reg.as_object_mut().unwrap();

    // Replace endpoint
    obj.insert("endpoint".to_string(), serde_json::Value::String(endpoint.to_string()));

    // Replace entity id inside information[0].entities[0].id
    if let Some(info) = obj.get_mut("information") {
        if let Some(first) = info.as_array_mut().and_then(|a| a.first_mut()) {
            if let Some(entities) = first.get_mut("entities") {
                if let Some(entity) = entities.as_array_mut().and_then(|a| a.first_mut()) {
                    if let Some(id_field) = entity.get_mut("id") {
                        *id_field = serde_json::Value::String(entity_id.to_string());
                    }
                }
            }
        }
    }

    // Set mode if provided
    if let Some(m) = mode {
        obj.insert("mode".to_string(), serde_json::Value::String(m.to_string()));
    }

    // Set operations if provided
    if let Some(ops) = operations {
        let ops_array: Vec<serde_json::Value> = ops.iter().map(|o| serde_json::Value::String(o.to_string())).collect();
        obj.insert("operations".to_string(), serde_json::Value::Array(ops_array));
    }

    // Set contextSourceAlias if provided
    if let Some(alias) = context_source_alias {
        obj.insert("contextSourceAlias".to_string(), serde_json::Value::String(alias.to_string()));
    }

    reg
}
