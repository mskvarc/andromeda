//! The append-only rule of the dictionaries.
//!
//! Dictionary keys are held where no foreign key reaches (sub-attribute
//! elements, queue records, every Snapshot schema's copies), so a key is
//! correct only while it names the same value for the life of the tenant. The
//! triggers that refuse every other change are what makes that true, so their
//! declared text is pinned here.

use crate::repository::{PinError, function_definition, schema, trigger_definition_on};
use pretty_assertions::assert_eq;

/// The trigger function every dictionary guard executes.
const REFUSAL: &str = "refuse_dictionary_change";

#[test]
fn both_name_dictionaries_refuse_update_delete_and_truncate() -> Result<(), PinError> {
    let schema = schema()?;
    for table in ["attribute_names", "entity_type_names"] {
        assert_eq!(
            trigger_definition_on(&schema, &format!("trg_{table}_append_only_row"), table)?,
            format!("before update or delete on {table} for each row execute function {REFUSAL} ()")
        );
        assert_eq!(
            trigger_definition_on(&schema, &format!("trg_{table}_append_only_truncate"), table)?,
            format!("before truncate on {table} for each statement execute function {REFUSAL} ()")
        );
    }
    let body = function_definition(&schema, REFUSAL)?;
    assert!(body.starts_with("() returns trigger language plpgsql"), "{body}");
    assert!(body.contains("raise restrict_violation"), "the refusal raises SQLSTATE 23001: {body}");
    assert!(body.contains("is append-only"), "{body}");
    Ok(())
}
