//! The JSON Schema of the configuration file, which editors validate an
//! `andromeda.toml` against.

use crate::{broker_config::BrokerConfig, reference::ReferenceError};
use schemars::{
    Schema,
    generate::SchemaSettings,
    transform::{RecursiveTransform, Transform},
};

/// The schema of the whole file, as editors validate it.
///
/// It is JSON Schema draft 7, the newest draft TOML editors read, with every
/// table written out in place so the schema reads top to bottom like the
/// file. It requires no key: the loader lays the file over the built-in
/// defaults and lays the environment over the file, so any key may be left
/// out of the file. A key that has no default, such as `database.connection`,
/// may come from the environment, and a key a table needs once the file
/// writes that table, such as `database.pooler.connection`, is refused by the
/// loader rather than by the schema.
#[must_use]
pub fn configuration_schema() -> Schema {
    let mut schema = declared_schema();
    RecursiveTransform(leave_every_key_optional).transform(&mut schema);
    schema
}

/// The schema as the types declare it: a table requires each key that is not
/// optional in its own type.
pub(crate) fn declared_schema() -> Schema {
    let mut settings = SchemaSettings::draft07();
    settings.inline_subschemas = true;
    settings.into_generator().into_root_schema_for::<BrokerConfig>()
}

/// The schema as pretty-printed JSON text, ending in a newline.
///
/// # Errors
///
/// Returns [`ReferenceError::Schema`] when the schema cannot be written.
pub fn configuration_schema_text() -> Result<String, ReferenceError> {
    let mut text = serde_json::to_string_pretty(&configuration_schema())?;
    text.push('\n');
    Ok(text)
}

/// Drop the keys a table requires.
fn leave_every_key_optional(schema: &mut Schema) {
    schema.remove("required");
}
