//! The reading of every object a migration declares, statement by statement.

use crate::{
    declared_schema::{
        column::{ColumnSource, CompressionSetting, DeclaredColumn},
        constraint::{ConstraintValidation, DeclaredConstraint},
        created_object::{CreatedObject, ObjectKind},
        do_block::DoBlock,
        domain::DeclaredDomain,
        function::{DeclaredFunction, definition_element},
        index::DeclaredIndex,
        parsed_migration::ParsedMigration,
        parser_node::{function_call, string_constant},
        qualified_name::QualifiedName,
        relation_reference::{CallPlace, FunctionCall, RelationReference},
        statement_context::StatementContext,
        statistics::DeclaredStatistics,
        syntax_walk::SyntaxSites,
        table::{DeclaredTable, Hypertable, ReplicaIdentity, ReplicaIdentityKind, StorageParameter},
        trigger::DeclaredTrigger,
    },
    plpgsql_tree::PlpgsqlError,
};
use pg_query::{
    Node,
    NodeEnum,
    protobuf::{AlterTableCmd, AlterTableStmt, AlterTableType, ColumnDef, CreateStmt, SelectStmt, Token},
};

/// The function that converts a table into a hypertable.
const CREATE_HYPERTABLE: &str = "create_hypertable";

/// The partitioning a hypertable's second argument names.
const BY_RANGE: &str = "by_range";

/// Every object the migrations declare, in the order written.
#[derive(Debug, Default)]
pub struct SchemaObjects {
    /// Every table.
    pub(crate) tables: Vec<DeclaredTable>,
    /// Every column of a table, every column added or retyped, and every field
    /// of a composite type.
    pub(crate) columns: Vec<DeclaredColumn>,
    /// Every compression method `alter column ... set compression` sets.
    pub(crate) compression_settings: Vec<CompressionSetting>,
    /// Every constraint of a table.
    pub(crate) constraints: Vec<DeclaredConstraint>,
    /// Every `validate constraint`.
    pub(crate) validations: Vec<ConstraintValidation>,
    /// Every storage parameter a table sets.
    pub(crate) storage_parameters: Vec<StorageParameter>,
    /// Every domain.
    pub(crate) domains: Vec<DeclaredDomain>,
    /// Every index.
    pub(crate) indexes: Vec<DeclaredIndex>,
    /// Every extended statistics object.
    pub(crate) statistics: Vec<DeclaredStatistics>,
    /// Every function.
    pub(crate) functions: Vec<DeclaredFunction>,
    /// Every `do` block.
    pub(crate) do_blocks: Vec<DoBlock>,
    /// Every trigger.
    pub(crate) triggers: Vec<DeclaredTrigger>,
    /// Every table converted into a hypertable.
    pub(crate) hypertables: Vec<Hypertable>,
    /// Every replica identity a table is given.
    pub(crate) replica_identities: Vec<ReplicaIdentity>,
    /// Every named object created.
    pub(crate) created: Vec<CreatedObject>,
    /// Every relation named without being created.
    pub(crate) references: Vec<RelationReference>,
    /// Every function called outside a function body.
    pub(crate) calls: Vec<FunctionCall>,
}

impl SchemaObjects {
    /// Read every statement of `migration`.
    ///
    /// # Errors
    ///
    /// Returns [`SchemaReadError`] when a routine body or an embedded
    /// expression cannot be read.
    pub fn read(&mut self, migration: &ParsedMigration) -> Result<(), SchemaReadError> {
        for statement in migration.statements() {
            let Some(node) = statement.node() else {
                continue;
            };
            let context = StatementContext::new(migration, statement.span());
            let sites = SyntaxSites::of(node).map_err(SchemaReadError::Walk)?;
            let created_relation = self.read_statement(node, &context)?;
            let common_tables = sites.common_tables();
            self.references.extend(
                sites
                    .relations()
                    .iter()
                    .filter(|relation| created_relation.is_none_or(|created| relation.offset() != Some(created)))
                    .filter(|relation| relation.name().schema().is_some() || !common_tables.iter().any(|common| common == relation.name().name()))
                    .map(|relation| RelationReference::new(relation.name().clone(), context.origin_at(location_of(relation.offset())))),
            );
            if !matches!(node, NodeEnum::CreateFunctionStmt(_)) {
                self.calls.extend(
                    sites
                        .calls()
                        .iter()
                        .map(|call| FunctionCall::new(call.name().clone(), CallPlace::Statement, context.origin_at(location_of(call.offset())))),
                );
            }
        }
        Ok(())
    }

    /// Read the objects `node` declares; the offset of the relation it creates
    /// is returned, so that relation is not also recorded as a reference.
    fn read_statement(&mut self, node: &NodeEnum, context: &StatementContext<'_>) -> Result<Option<usize>, SchemaReadError> {
        if let NodeEnum::CreateStmt(create) = node {
            return Ok(self.read_create_table(create, context));
        }
        if let NodeEnum::AlterTableStmt(alter) = node {
            self.read_alter_table(alter, context);
        } else if let NodeEnum::CompositeTypeStmt(composite) = node {
            let relation = composite.typevar.as_ref();
            let name = relation.map_or_default(|relation| relation.relname.as_str());
            let location = relation.map_or(-1, |relation| relation.location);
            self.created
                .push(CreatedObject::new(ObjectKind::CompositeType, name, context.origin_at(location)));
            for definition in composite.coldeflist.iter().filter_map(column_definition) {
                self.add_column(definition, &definition.colname, name, ColumnSource::CompositeType, context);
            }
            return Ok(relation.and_then(|relation| usize::try_from(relation.location).ok()));
        } else if let NodeEnum::CreateDomainStmt(domain) = node
            && let Some(domain) = DeclaredDomain::read(domain, context)
        {
            self.created.push(CreatedObject::new(ObjectKind::Domain, domain.name(), domain.origin()));
            self.domains.push(domain);
        } else if let NodeEnum::IndexStmt(index) = node {
            let index = DeclaredIndex::read(index, context).map_err(SchemaReadError::Walk)?;
            self.created.push(CreatedObject::new(ObjectKind::Index, index.name(), index.origin()));
            self.indexes.push(index);
        } else if let NodeEnum::CreateStatsStmt(statistics) = node
            && let Some(statistics) = DeclaredStatistics::read(statistics, context)
        {
            self.created
                .push(CreatedObject::new(ObjectKind::Statistics, statistics.name(), statistics.origin()));
            self.statistics.push(statistics);
        } else if let NodeEnum::CreateFunctionStmt(function) = node
            && let Some(function) = DeclaredFunction::read(function, context).map_err(SchemaReadError::Routine)?
        {
            self.created
                .push(CreatedObject::new(ObjectKind::Function, function.name().name(), function.origin()));
            self.functions.push(function);
        } else if let NodeEnum::CreateTrigStmt(trigger) = node
            && let Some(trigger) = DeclaredTrigger::read(trigger, context)
        {
            self.created.push(CreatedObject::new(ObjectKind::Trigger, trigger.name(), trigger.origin()));
            self.calls.push(FunctionCall::new(
                trigger.function().clone(),
                CallPlace::TriggerTarget,
                trigger.function_origin(),
            ));
            self.triggers.push(trigger);
        } else if let NodeEnum::DoStmt(block) = node {
            let block = DoBlock::read(block, context).map_err(SchemaReadError::Routine)?;
            self.read_do_block_calls(&block)?;
            self.do_blocks.push(block);
        } else if let NodeEnum::ViewStmt(view) = node
            && let Some(relation) = view.view.as_ref()
        {
            self.created.push(CreatedObject::new(
                ObjectKind::View,
                relation.relname.as_str(),
                context.origin_at(relation.location),
            ));
            return Ok(usize::try_from(relation.location).ok());
        } else if let NodeEnum::CreateExtensionStmt(extension) = node {
            let origin = context.origin_of_name(&extension.extname);
            self.created.push(CreatedObject::new(ObjectKind::Extension, extension.extname.as_str(), origin));
        } else if let NodeEnum::SelectStmt(select) = node {
            self.read_hypertables(select, context);
        }
        Ok(None)
    }

    /// Read a `create table`, returning the offset of the table's name.
    fn read_create_table(&mut self, create: &CreateStmt, context: &StatementContext<'_>) -> Option<usize> {
        let relation = create.relation.as_ref()?;
        let table = relation.relname.as_str();
        let origin = context.origin_at(relation.location);
        self.tables.push(DeclaredTable::new(table, origin));
        self.created.push(CreatedObject::new(ObjectKind::Table, table, origin));
        for element in &create.table_elts {
            match &element.node {
                Some(NodeEnum::ColumnDef(definition)) => self.add_column(definition, &definition.colname, table, ColumnSource::CreateTable, context),
                Some(NodeEnum::Constraint(constraint)) => self.add_constraint(DeclaredConstraint::read(constraint, table, None, context)),
                Some(_) | None => {}
            }
        }
        for option in create.options.iter().filter_map(definition_element) {
            let after_name = context.after_name(context.origin_at(option.location).offset(), &option.defname);
            let after_equals = context
                .migration()
                .code_tokens(after_name..context.span().end)
                .next()
                .filter(|token| token.kind() == Token::Ascii61)
                .map_or(after_name, |equals| equals.range().end);
            let value = context.migration().text()[context.item_from(after_equals)]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            self.storage_parameters.push(StorageParameter::new(table, option.defname.as_str(), value));
        }
        usize::try_from(relation.location).ok()
    }

    /// Read an `alter table`: the columns it adds or retypes, the compression
    /// and the replica identity it sets and the constraints it adds or
    /// validates.
    fn read_alter_table(&mut self, alter: &AlterTableStmt, context: &StatementContext<'_>) {
        let table = alter.relation.as_ref().map_or_default(|relation| relation.relname.as_str());
        for command in alter.cmds.iter().filter_map(|node| match &node.node {
            Some(NodeEnum::AlterTableCmd(command)) => Some(command),
            Some(_) | None => None,
        }) {
            self.read_alter_table_command(command, table, context);
        }
    }

    /// Read one action of an `alter table`.
    fn read_alter_table_command(&mut self, command: &AlterTableCmd, table: &str, context: &StatementContext<'_>) {
        let definition = command.def.as_ref().and_then(|definition| definition.node.as_ref());
        match (AlterTableType::try_from(command.subtype), definition) {
            (Ok(AlterTableType::AtAddColumn), Some(NodeEnum::ColumnDef(column))) => {
                self.add_column(column, &column.colname, table, ColumnSource::AddColumn, context);
            }
            (Ok(AlterTableType::AtAlterColumnType), Some(NodeEnum::ColumnDef(column))) => {
                if let Some(column) = DeclaredColumn::read(column, &command.name, table, ColumnSource::AlterColumnType, context) {
                    self.columns.push(column);
                }
            }
            (Ok(AlterTableType::AtAddConstraint), Some(NodeEnum::Constraint(constraint))) => {
                self.add_constraint(DeclaredConstraint::read(constraint, table, None, context));
            }
            (Ok(AlterTableType::AtValidateConstraint), None) => {
                self.validations
                    .push(ConstraintValidation::new(table, &command.name, context.origin_of_name(&command.name)));
            }
            (Ok(AlterTableType::AtSetCompression), Some(NodeEnum::String(method))) => {
                self.compression_settings.push(CompressionSetting::new(
                    table,
                    &command.name,
                    &method.sval,
                    context.origin_of_name(&command.name),
                ));
            }
            (Ok(AlterTableType::AtReplicaIdentity), Some(NodeEnum::ReplicaIdentityStmt(identity))) => {
                if let Ok(kind) = identity.identity_type.parse::<ReplicaIdentityKind>() {
                    self.replica_identities.push(ReplicaIdentity::new(table, kind));
                }
            }
            (Ok(_) | Err(_), _) => {}
        }
    }

    /// Record the column `definition` declares in `relation` as `name`, with
    /// the constraints written on it.
    fn add_column(&mut self, definition: &ColumnDef, name: &str, relation: &str, source: ColumnSource, context: &StatementContext<'_>) {
        let Some(column) = DeclaredColumn::read(definition, name, relation, source, context) else {
            return;
        };
        self.created.push(CreatedObject::new(ObjectKind::Column, name, column.origin()));
        for constraint in definition.constraints.iter().filter_map(|node| match &node.node {
            Some(NodeEnum::Constraint(constraint)) => Some(constraint),
            Some(_) | None => None,
        }) {
            self.add_constraint(DeclaredConstraint::read(constraint, relation, Some(name), context));
        }
        self.columns.push(column);
    }

    /// Record `constraint`, and its name when it has one.
    fn add_constraint(&mut self, constraint: Option<DeclaredConstraint>) {
        let Some(constraint) = constraint else {
            return;
        };
        if let Some(name) = constraint.name() {
            self.created.push(CreatedObject::new(ObjectKind::Constraint, name, constraint.origin()));
        }
        self.constraints.push(constraint);
    }

    /// Record every `select create_hypertable('table', by_range('column'))`.
    fn read_hypertables(&mut self, select: &SelectStmt, context: &StatementContext<'_>) {
        for target in &select.target_list {
            let Some(NodeEnum::ResTarget(target)) = &target.node else {
                continue;
            };
            let Some(call) = target.val.as_deref().and_then(function_call) else {
                continue;
            };
            if QualifiedName::of_nodes(&call.funcname).is_none_or(|name| name.name() != CREATE_HYPERTABLE) {
                continue;
            }
            let Some(table) = call.args.first().and_then(string_constant) else {
                continue;
            };
            let partition_column = call.args.get(1).and_then(|partitioning| {
                string_constant(partitioning).or_else(|| {
                    function_call(partitioning)
                        .filter(|partitioning| QualifiedName::of_nodes(&partitioning.funcname).is_some_and(|name| name.name() == BY_RANGE))
                        .and_then(|partitioning| partitioning.args.first())
                        .and_then(string_constant)
                })
            });
            self.hypertables.push(Hypertable::new(
                table.to_owned(),
                partition_column.map(str::to_owned),
                context.origin_at(call.location),
            ));
        }
    }

    /// Record every function the expressions of `block` call.
    fn read_do_block_calls(&mut self, block: &DoBlock) -> Result<(), SchemaReadError> {
        for routine in block.routines() {
            for sql in routine.embedded_sql() {
                let parsed = pg_query::parse(sql.query(), sql.parse_options()).map_err(SchemaReadError::Expression)?;
                for statement in parsed.protobuf.stmts.iter().filter_map(|statement| statement.stmt.as_deref()) {
                    let sites = SyntaxSites::of(statement).map_err(SchemaReadError::Walk)?;
                    self.calls.extend(
                        sites
                            .calls()
                            .iter()
                            .map(|call| FunctionCall::new(call.name().clone(), CallPlace::DoBlock, block.origin())),
                    );
                }
            }
        }
        Ok(())
    }
}

/// The parser location of `offset`, -1 when unknown.
fn location_of(offset: Option<usize>) -> i32 {
    offset.and_then(|offset| i32::try_from(offset).ok()).unwrap_or(-1)
}

/// The column definition `node` holds.
fn column_definition(node: &Node) -> Option<&ColumnDef> {
    match &node.node {
        Some(NodeEnum::ColumnDef(definition)) => Some(definition),
        Some(_) | None => None,
    }
}

/// Why the objects of a migration could not be read.
#[derive(Debug, thiserror::Error)]
pub enum SchemaReadError {
    /// A routine body could not be read.
    #[error(transparent)]
    Routine(#[from] PlpgsqlError),
    /// An expression a `do` block embeds does not parse.
    #[error("an expression of a do block does not parse: {0}")]
    Expression(pg_query::Error),
    /// A syntax tree could not be walked.
    #[error("a syntax tree could not be walked: {0}")]
    Walk(serde_json::Error),
}
