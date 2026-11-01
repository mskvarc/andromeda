//! The schema the migrations declare, read from PostgreSQL's own syntax tree
//! of every migration.
//!
//! Every rule and every test of the schema reads this one model, so they all
//! agree on what the migrations contain: the tables and their replica
//! identities, columns, composite types, domains, constraints, indexes,
//! statistics objects, functions, triggers and `do` blocks, every relation
//! named and every function called, and every token, the tokens of routine
//! bodies included.

/// The bounds a check expression puts on its columns.
pub mod check_bound;
/// A column of a table or a field of a composite type.
pub mod column;
/// A constraint of a table.
pub mod constraint;
/// A named object a migration creates.
pub mod created_object;
/// The data type a column, a field or a domain is declared with.
pub mod declared_type;
/// An anonymous code block.
pub mod do_block;
/// A domain.
pub mod domain;
/// A function.
pub mod function;
/// An index.
pub mod index;
/// Where a declared object is written.
pub mod origin;
/// One migration as the rules read it.
pub mod parsed_migration;
/// One parser node read as the kind it holds.
pub mod parser_node;
/// A possibly schema-qualified object name.
pub mod qualified_name;
/// A relation a statement names, and a function it calls.
pub mod relation_reference;
/// The reading of every object a migration declares.
pub mod schema_reader;
/// The statement a declared object is read from.
pub mod statement_context;
/// An extended statistics object.
pub mod statistics;
/// Every call, relation and column a syntax tree holds.
pub mod syntax_walk;
/// A table, its storage parameters, its replica identity and its conversion
/// to a hypertable.
pub mod table;
/// Runs of consecutive code tokens.
pub mod token_sequence;
/// A trigger.
pub mod trigger;

use crate::{
    declared_schema::{
        column::{CompressionSetting, DeclaredColumn},
        constraint::{ConstraintValidation, DeclaredConstraint},
        created_object::{CreatedObject, ObjectKind},
        declared_type::{BuiltinType, DeclaredType, IntegerWidth, Storage},
        do_block::DoBlock,
        domain::DeclaredDomain,
        function::DeclaredFunction,
        index::DeclaredIndex,
        origin::Origin,
        parsed_migration::{MigrationParseError, ParsedMigration},
        relation_reference::{FunctionCall, RelationReference},
        schema_reader::{SchemaObjects, SchemaReadError},
        statistics::DeclaredStatistics,
        table::{DeclaredTable, Hypertable, ReplicaIdentity, StorageParameter},
        trigger::DeclaredTrigger,
    },
    migration_file::MigrationFile,
    migration_set::MigrationSet,
    migration_version::MigrationVersion,
};
use std::{ops::Range, sync::Arc};

/// What a declared type is, once every domain is traced to its base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedType {
    /// A built-in or extension type, or an array of one.
    Builtin {
        /// The element type.
        element: BuiltinType,
        /// Whether the value is an array of it.
        array: bool,
    },
    /// A composite type a migration creates, or an array of one.
    Composite,
    /// A type the migrations neither create nor this model knows.
    Unknown,
}

/// The schema every migration declares.
#[derive(Debug)]
pub struct DeclaredSchema {
    /// Every migration, in version order.
    migrations: Vec<ParsedMigration>,
    /// Every object they declare.
    objects: SchemaObjects,
}

impl DeclaredSchema {
    /// Read the schema `migrations`, each file with its text, declare.
    ///
    /// # Errors
    ///
    /// Returns [`DeclaredSchemaError`] naming the first migration that does
    /// not parse or whose objects cannot be read.
    pub fn read(migrations: impl IntoIterator<Item = (MigrationFile, Arc<str>)>) -> Result<DeclaredSchema, DeclaredSchemaError> {
        let mut parsed = migrations
            .into_iter()
            .map(|(file, text)| match ParsedMigration::parse(file.clone(), text) {
                Ok(migration) => Ok(migration),
                Err(source) => Err(DeclaredSchemaError::Unparsed { file: Box::new(file), source }),
            })
            .collect::<Result<Vec<_>, _>>()?;
        parsed.sort_by_key(|migration| migration.file().version());
        let mut objects = SchemaObjects::default();
        for migration in &parsed {
            objects.read(migration).map_err(|source| DeclaredSchemaError::Unread {
                // clone: the error names the file the schema keeps
                file: Box::new(migration.file().clone()),
                source,
            })?;
        }
        Ok(DeclaredSchema { migrations: parsed, objects })
    }

    /// Every migration, in version order.
    #[must_use]
    pub fn migrations(&self) -> &[ParsedMigration] {
        &self.migrations
    }

    /// The migration at `version`.
    #[must_use]
    pub fn migration(&self, version: MigrationVersion) -> Option<&ParsedMigration> {
        self.migrations
            .binary_search_by_key(&version, |migration| migration.file().version())
            .ok()
            .and_then(|position| self.migrations.get(position))
    }

    /// The set of the migration `origin` is in.
    #[must_use]
    pub fn set_of(&self, origin: Origin) -> Option<MigrationSet> {
        self.migration(origin.version()).map(|migration| migration.file().set())
    }

    /// The text of `bytes` of the migration `origin` is in.
    #[must_use]
    pub fn text(&self, origin: Origin, bytes: Range<usize>) -> &str {
        self.migration(origin.version())
            .and_then(|migration| migration.text().get(bytes))
            .unwrap_or_default()
    }

    /// Every table.
    #[must_use]
    pub fn tables(&self) -> &[DeclaredTable] {
        &self.objects.tables
    }

    /// Every column of a table, every column added or retyped, and every field
    /// of a composite type, in the order written.
    #[must_use]
    pub fn columns(&self) -> &[DeclaredColumn] {
        &self.objects.columns
    }

    /// Every compression method `alter column ... set compression` sets, in
    /// the order written.
    #[must_use]
    pub fn compression_settings(&self) -> &[CompressionSetting] {
        &self.objects.compression_settings
    }

    /// Every constraint of a table.
    #[must_use]
    pub fn constraints(&self) -> &[DeclaredConstraint] {
        &self.objects.constraints
    }

    /// Every storage parameter a table sets, in the order written.
    #[must_use]
    pub fn storage_parameters(&self) -> &[StorageParameter] {
        &self.objects.storage_parameters
    }

    /// Every `validate constraint`, in the order written.
    #[must_use]
    pub fn validations(&self) -> &[ConstraintValidation] {
        &self.objects.validations
    }

    /// Every domain.
    #[must_use]
    pub fn domains(&self) -> &[DeclaredDomain] {
        &self.objects.domains
    }

    /// Every index.
    #[must_use]
    pub fn indexes(&self) -> &[DeclaredIndex] {
        &self.objects.indexes
    }

    /// Every extended statistics object.
    #[must_use]
    pub fn statistics(&self) -> &[DeclaredStatistics] {
        &self.objects.statistics
    }

    /// Every function.
    #[must_use]
    pub fn functions(&self) -> &[DeclaredFunction] {
        &self.objects.functions
    }

    /// Every `do` block.
    #[must_use]
    pub fn do_blocks(&self) -> &[DoBlock] {
        &self.objects.do_blocks
    }

    /// Every trigger.
    #[must_use]
    pub fn triggers(&self) -> &[DeclaredTrigger] {
        &self.objects.triggers
    }

    /// Every table converted into a hypertable.
    #[must_use]
    pub fn hypertables(&self) -> &[Hypertable] {
        &self.objects.hypertables
    }

    /// Every replica identity a table is given.
    #[must_use]
    pub fn replica_identities(&self) -> &[ReplicaIdentity] {
        &self.objects.replica_identities
    }

    /// Every named object created.
    #[must_use]
    pub fn created(&self) -> &[CreatedObject] {
        &self.objects.created
    }

    /// Every relation named without being created.
    #[must_use]
    pub fn references(&self) -> &[RelationReference] {
        &self.objects.references
    }

    /// Every function called outside a function body.
    #[must_use]
    pub fn calls(&self) -> &[FunctionCall] {
        &self.objects.calls
    }

    /// Whether a migration creates a composite type named `name`.
    #[must_use]
    pub fn is_composite_type(&self, name: &str) -> bool {
        self.objects
            .created
            .iter()
            .any(|object| object.kind() == ObjectKind::CompositeType && object.name() == name)
    }

    /// What `declared` is, every domain traced to its base.
    #[must_use]
    pub fn resolve(&self, declared: &DeclaredType) -> ResolvedType {
        let mut current = declared;
        let mut array = declared.is_array();
        // A domain over a domain is followed at most once per domain, so a
        // cycle (which PostgreSQL refuses anyway) cannot loop.
        for _ in 0..=self.objects.domains.len() {
            if let Some(element) = current.builtin() {
                return ResolvedType::Builtin { element, array };
            }
            let name = current.name().name();
            if self.is_composite_type(name) {
                return ResolvedType::Composite;
            }
            let Some(domain) = self.objects.domains.iter().find(|domain| domain.name() == name) else {
                return ResolvedType::Unknown;
            };
            current = domain.base();
            array |= current.is_array();
        }
        ResolvedType::Unknown
    }

    /// How a value of `declared` is stored, when the type is known.
    #[must_use]
    pub fn storage(&self, declared: &DeclaredType) -> Option<Storage> {
        match self.resolve(declared) {
            ResolvedType::Builtin { array: true, .. } | ResolvedType::Composite => Some(Storage::VariableLength),
            ResolvedType::Builtin { element, array: false } => Some(element.storage()),
            ResolvedType::Unknown => None,
        }
    }

    /// Whether a value of `declared` carries a collation.
    #[must_use]
    pub fn is_collatable(&self, declared: &DeclaredType) -> bool {
        match self.resolve(declared) {
            ResolvedType::Builtin { element, .. } => element.is_collatable(),
            ResolvedType::Composite | ResolvedType::Unknown => false,
        }
    }

    /// The integer width of `declared`'s elements, when it is an integer or
    /// an array of integers.
    #[must_use]
    pub fn resolve_integer_element(&self, declared: &DeclaredType) -> Option<IntegerWidth> {
        match self.resolve(declared) {
            ResolvedType::Builtin { element, .. } => element.integer_width(),
            ResolvedType::Composite | ResolvedType::Unknown => None,
        }
    }

    /// The width of `declared`, when it is a scalar integer.
    #[must_use]
    pub fn integer_width(&self, declared: &DeclaredType) -> Option<IntegerWidth> {
        match self.resolve(declared) {
            ResolvedType::Builtin { element, array: false } => element.integer_width(),
            ResolvedType::Builtin { array: true, .. } | ResolvedType::Composite | ResolvedType::Unknown => None,
        }
    }
}

/// Why the schema could not be read.
#[derive(Debug, thiserror::Error)]
pub enum DeclaredSchemaError {
    /// A migration does not parse or scan.
    #[error("{file} could not be read: {source}")]
    Unparsed {
        /// The migration concerned.
        file: Box<MigrationFile>,
        /// Why it could not be read.
        source: MigrationParseError,
    },
    /// The objects of a migration could not be read.
    #[error("the objects {file} declares could not be read: {source}")]
    Unread {
        /// The migration concerned.
        file: Box<MigrationFile>,
        /// Why they could not be read.
        source: SchemaReadError,
    },
}
