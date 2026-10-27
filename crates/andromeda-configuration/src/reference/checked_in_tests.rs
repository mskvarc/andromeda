use crate::reference::{ReferenceError, descriptor::configuration_descriptor_text, json_schema::configuration_schema_text, template::configuration_template};
use andromeda_architecture_checks::generated_artifact::assert_checked_in;

/// The repository root, relative to which the checked-in copies lie.
const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

#[test]
fn the_checked_in_template_is_the_generated_one() -> Result<(), ReferenceError> {
    assert_checked_in(WORKSPACE_ROOT, "docs/configuration/andromeda.toml", &configuration_template()?);
    Ok(())
}

#[test]
fn the_checked_in_schema_is_the_generated_one() -> Result<(), ReferenceError> {
    assert_checked_in(WORKSPACE_ROOT, "docs/configuration/andromeda.schema.json", &configuration_schema_text()?);
    Ok(())
}

#[test]
fn the_checked_in_descriptor_is_the_generated_one() -> Result<(), ReferenceError> {
    assert_checked_in(
        WORKSPACE_ROOT,
        "docs/configuration/andromeda.descriptor.json",
        &configuration_descriptor_text()?,
    );
    Ok(())
}
