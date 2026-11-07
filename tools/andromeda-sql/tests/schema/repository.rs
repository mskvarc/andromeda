//! The committed migrations read into the declared schema, and the lookups
//! every pin test shares.

use andromeda_sql::{
    declared_schema::{
        DeclaredSchema,
        DeclaredSchemaError,
        constraint::DeclaredConstraint,
        function::DeclaredFunction,
        index::DeclaredIndex,
        origin::Origin,
        table::StorageParameter,
        trigger::DeclaredTrigger,
    },
    migration_tree::{MigrationTree, MigrationTreeError},
};
use std::{fs, io, ops::Range, path::Path, sync::Arc};

/// Why a pin test could not find what it pins.
#[derive(Debug, thiserror::Error)]
pub(crate) enum PinError {
    /// The migrations could not be listed.
    #[error(transparent)]
    Tree(#[from] MigrationTreeError),
    /// A migration could not be read.
    #[error(transparent)]
    Read(#[from] io::Error),
    /// The schema could not be read.
    #[error(transparent)]
    Schema(#[from] DeclaredSchemaError),
    /// No migration declares the object a test pins.
    #[error("no migration declares {0}")]
    Missing(String),
}

/// The schema the committed migrations declare.
pub(crate) fn schema() -> Result<DeclaredSchema, PinError> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let migrations = MigrationTree::read(&root)?
        .into_files()
        .map(|file| {
            let text = fs::read_to_string(file.path())?;
            Ok((file, Arc::<str>::from(text)))
        })
        .collect::<Result<Vec<_>, io::Error>>()?;
    Ok(DeclaredSchema::read(migrations)?)
}

/// `text` with every run of whitespace collapsed to one space.
pub(crate) fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The text of `bytes` of the migration `origin` is in, its whitespace
/// collapsed.
pub(crate) fn written(schema: &DeclaredSchema, origin: Origin, bytes: Range<usize>) -> String {
    normalized(schema.text(origin, bytes))
}

/// The constraint `name`.
pub(crate) fn constraint<'schema>(schema: &'schema DeclaredSchema, name: &str) -> Result<&'schema DeclaredConstraint, PinError> {
    schema
        .constraints()
        .iter()
        .find(|constraint| constraint.name() == Some(name))
        .ok_or_else(|| PinError::Missing(name.to_owned()))
}

/// The definition of the constraint `name` after its name.
pub(crate) fn constraint_definition(schema: &DeclaredSchema, name: &str) -> Result<String, PinError> {
    let constraint = constraint(schema, name)?;
    Ok(written(schema, constraint.origin(), constraint.definition()))
}

/// The index `name`.
pub(crate) fn index<'schema>(schema: &'schema DeclaredSchema, name: &str) -> Result<&'schema DeclaredIndex, PinError> {
    schema
        .indexes()
        .iter()
        .find(|index| index.name() == name)
        .ok_or_else(|| PinError::Missing(name.to_owned()))
}

/// The definition of the index `name` after its name.
pub(crate) fn index_definition(schema: &DeclaredSchema, name: &str) -> Result<String, PinError> {
    let index = index(schema, name)?;
    Ok(written(schema, index.origin(), index.definition()))
}

/// The trigger `name`.
pub(crate) fn trigger<'schema>(schema: &'schema DeclaredSchema, name: &str) -> Result<&'schema DeclaredTrigger, PinError> {
    schema
        .triggers()
        .iter()
        .find(|trigger| trigger.name() == name)
        .ok_or_else(|| PinError::Missing(name.to_owned()))
}

/// The definition of the trigger `name` after its name, which must fire on
/// `table`.
pub(crate) fn trigger_definition_on(schema: &DeclaredSchema, name: &str, table: &str) -> Result<String, PinError> {
    let trigger = trigger(schema, name)?;
    pretty_assertions::assert_eq!(trigger.table(), table, "{name} fires on the wrong table");
    Ok(written(schema, trigger.origin(), trigger.definition()))
}

/// The one function named `name`.
pub(crate) fn function<'schema>(schema: &'schema DeclaredSchema, name: &str) -> Result<&'schema DeclaredFunction, PinError> {
    let mut named = schema.functions().iter().filter(|function| function.name().name() == name);
    match (named.next(), named.next()) {
        (Some(function), None) => Ok(function),
        (Some(_), Some(_)) | (None, _) => Err(PinError::Missing(format!("exactly one function {name}"))),
    }
}

/// The definition of the one function named `name` after its name.
pub(crate) fn function_definition(schema: &DeclaredSchema, name: &str) -> Result<String, PinError> {
    let function = function(schema, name)?;
    Ok(written(schema, function.origin(), function.definition()))
}

/// The value `table` declares for storage parameter `name`, if any.
pub(crate) fn parameter<'schema>(schema: &'schema DeclaredSchema, table: &str, name: &str) -> Option<&'schema str> {
    schema
        .storage_parameters()
        .iter()
        .find(|parameter| parameter.table() == table && parameter.name() == name)
        .map(StorageParameter::value)
}
