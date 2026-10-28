//! Andromeda's own lint rules: the conventions the schema depends on that
//! Squawk cannot express, each checked on the declared schema.
//!
//! A rule reads only SQL: the migrations, through PostgreSQL's syntax tree and
//! token scan, the formatter's vocabulary files, and the seal ledger saying
//! which migrations a release has shipped. Every exception a rule
//! makes is data in the rule's own module; a migration cannot switch a rule
//! off.

/// A boolean column reads as a predicate.
pub mod boolean_not_predicate;
/// A size check is named `_length`, not `_bound`.
pub mod bound_suffix;
/// A text value is stored and compared under the `C` collation.
pub mod c_collation;
/// An index serving a queue claim is named `_claim`.
pub mod claim_index_suffix;
/// The comments of a migration follow its header template and prose rules.
pub mod comment_style;
/// A foreign key or check added to a sealed table is validated later.
pub mod constraint_on_sealed_table;
/// No fixed-width column follows a variable-length one.
pub mod fixed_width_before_variable;
/// No name carries an abbreviation or a storage-type word.
pub mod forbidden_word;
/// A foreign-key column is named like the column it references.
pub mod foreign_key_column_name;
/// A foreign key's columns lead an index.
pub mod foreign_key_indexed;
/// Every called function is known to `pg_format`, every vocabulary word used.
pub mod formatter_vocabulary;
/// A function body is bound when created, and names relations qualified.
pub mod function_body_bound;
/// Columns rewritten on every write stay out of every index.
pub mod heap_only_columns;
/// A hypertable has no foreign key and no unique key without its partition column.
pub mod hypertable_shape;
/// An array of keys is named `_keys`, not `_ids`.
pub mod identifier_array_not_keys;
/// Every created name fits a PostgreSQL identifier.
pub mod identifier_length;
/// An identity column is named `_key`.
pub mod identity_not_key;
/// No pattern operator class, hash call or redundant prefix index.
pub mod index_hygiene;
/// An integer named `id` is assigned outside the database.
pub mod integer_identifier_not_key;
/// An integer column is a key, an ordinal, a version or a bounded counter.
pub mod integer_width;
/// Every text key of a B-tree is bounded below the entry limit.
pub mod key_length_bound;
/// No code token holds an uppercase letter.
pub mod lowercase;
/// A column of a compressible type declares `compression lz4`.
pub mod lz4_compression;
/// The words the naming rules read.
pub mod naming_vocabulary;
/// No exception handler catches `others`.
pub mod no_catch_all_exception;
/// No object is created `or replace`.
pub mod no_or_replace;
/// No function runs as its owner.
pub mod no_security_definer;
/// A constraint, index or statistics name starts with its kind and table.
pub mod object_prefix;
/// The fixtures the rule tests build their schemas from.
#[cfg(test)]
pub mod rule_fixture;
/// Each migration set references only the relations it may reach.
pub mod set_isolation;
/// A trigger is named `trg_<table>_<what>`.
pub mod trigger_name;
/// A unique constraint over a write identity is named `_write_identity`.
pub mod write_identity_suffix;

use crate::{
    declared_schema::{DeclaredSchema, origin::Origin},
    formatter_vocabulary::{FormatterVocabulary, VocabularyFile},
    lint_finding::{LintFinding, LintRule, LintSeverity},
    linted_source::SourceKey,
    seal_ledger::SealLedger,
    source_range::SourceRange,
};
use std::{fmt, ops::Range};
use strum::{EnumMessage, IntoEnumIterator};

/// One of Andromeda's lint rules, named as a finding names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, strum::Display, strum::EnumIter, strum::EnumString, strum::EnumMessage)]
#[strum(serialize_all = "kebab-case")]
pub enum Rule {
    /// A column of a compressible type declares `compression lz4`.
    #[strum(message = "declare `compression lz4` on the column")]
    Lz4Compression,
    /// A text value is stored and compared under the `C` collation.
    #[strum(message = "declare `collate \"C\"`, the one collation a migration names")]
    CCollation,
    /// An integer column is a key, an ordinal, a version or a bounded counter.
    #[strum(message = "use `bigint`, or bound a `smallint` with a check")]
    IntegerWidth,
    /// Each migration set references only the relations it may reach.
    #[strum(message = "reference only relations the migration's own set can reach")]
    SetIsolation,
    /// A foreign key's columns lead an index.
    #[strum(message = "index the referencing columns, or key the table on them")]
    ForeignKeyIndexed,
    /// A hypertable has no foreign key and no unique key without its partition
    /// column.
    #[strum(message = "drop the foreign key, or add the partition column to the unique key")]
    HypertableShape,
    /// A function body is bound when created, and names relations qualified.
    #[strum(message = "write a SQL-standard body, or qualify the relation, through `%I` and `tg_table_schema` when it is dynamic")]
    FunctionBodyBound,
    /// No object is created `or replace`.
    #[strum(message = "create the object plainly; a migration runs once")]
    NoOrReplace,
    /// No exception handler catches `others`.
    #[strum(message = "catch the conditions expected, or test for what the block needs first")]
    NoCatchAllException,
    /// No function runs as its owner.
    #[strum(message = "let the function run with the caller's privileges")]
    NoSecurityDefiner,
    /// No code token holds an uppercase letter.
    #[strum(message = "write the token in lowercase")]
    Lowercase,
    /// Every called function is known to `pg_format`, every vocabulary word
    /// used.
    #[strum(message = "list the function in `.pg_format_functions`, or remove the word no migration uses")]
    FormatterVocabulary,
    /// Every created name fits a PostgreSQL identifier.
    #[strum(message = "shorten the name to 63 bytes, which PostgreSQL keeps whole")]
    IdentifierLength,
    /// The comments of a migration follow its header template and prose rules.
    #[strum(message = "keep the header template and write comments that stand alone")]
    CommentStyle,
    /// Every text key of a B-tree is bounded below the entry limit.
    #[strum(message = "add a check `octet_length(column) <= n` with n within the B-tree entry limit")]
    KeyLengthBound,
    /// No fixed-width column follows a variable-length one.
    #[strum(message = "move the fixed-width column before every variable-length one")]
    FixedWidthBeforeVariable,
    /// No pattern operator class, hash call or redundant prefix index.
    #[strum(message = "drop the operator class, the hash call or the redundant index")]
    IndexHygiene,
    /// Columns rewritten on every write stay out of every index.
    #[strum(message = "keep the column out of the index, so its updates stay heap-only")]
    HeapOnlyColumns,
    /// A constraint, index or statistics name starts with its kind and table.
    #[strum(message = "name it `<kind>_<table or its abbreviation>_<what>`")]
    ObjectPrefix,
    /// No name carries an abbreviation or a storage-type word.
    #[strum(message = "spell the word out, and name what the value means rather than how it is stored")]
    ForbiddenWord,
    /// A size check is named `_length`, not `_bound`.
    #[strum(message = "name the check `_length`")]
    BoundSuffix,
    /// An index serving a queue claim is named `_claim`.
    #[strum(message = "end the index name with `_claim`")]
    ClaimIndexSuffix,
    /// A unique constraint over a write identity is named `_write_identity`.
    #[strum(message = "end the constraint name with `_write_identity`")]
    WriteIdentitySuffix,
    /// An identity column is named `_key`.
    #[strum(message = "end the identity column's name with `_key`")]
    IdentityNotKey,
    /// An integer named `id` is assigned outside the database.
    #[strum(message = "name a database-generated integer `_key`")]
    IntegerIdentifierNotKey,
    /// An array of keys is named `_keys`, not `_ids`.
    #[strum(message = "name an array of database-generated integers `_keys`")]
    IdentifierArrayNotKeys,
    /// A foreign-key column is named like the column it references.
    #[strum(message = "name the referencing column after the column it references")]
    ForeignKeyColumnName,
    /// A boolean column reads as a predicate.
    #[strum(message = "name the boolean `is_<what>`")]
    BooleanNotPredicate,
    /// A trigger is named `trg_<table>_<what>`.
    #[strum(message = "name the trigger `trg_<table>_<what>` after the full table name")]
    TriggerName,
    /// A foreign key or check added to a sealed table is validated later.
    #[strum(message = "add it as a named table constraint `not valid`, and `validate constraint` it in a later migration")]
    ConstraintOnSealedTable,
}

impl Rule {
    /// What the rule finds in `context`.
    #[must_use]
    pub fn check(self, context: &RuleContext<'_>) -> Vec<RuleFinding> {
        let check = match self {
            Rule::Lz4Compression => lz4_compression::check,
            Rule::CCollation => c_collation::check,
            Rule::IntegerWidth => integer_width::check,
            Rule::SetIsolation => set_isolation::check,
            Rule::ForeignKeyIndexed => foreign_key_indexed::check,
            Rule::HypertableShape => hypertable_shape::check,
            Rule::FunctionBodyBound => function_body_bound::check,
            Rule::NoOrReplace => no_or_replace::check,
            Rule::NoCatchAllException => no_catch_all_exception::check,
            Rule::NoSecurityDefiner => no_security_definer::check,
            Rule::Lowercase => lowercase::check,
            Rule::FormatterVocabulary => formatter_vocabulary::check,
            Rule::IdentifierLength => identifier_length::check,
            Rule::CommentStyle => comment_style::check,
            Rule::KeyLengthBound => key_length_bound::check,
            Rule::FixedWidthBeforeVariable => fixed_width_before_variable::check,
            Rule::IndexHygiene => index_hygiene::check,
            Rule::HeapOnlyColumns => heap_only_columns::check,
            Rule::ObjectPrefix => object_prefix::check,
            Rule::ForbiddenWord => forbidden_word::check,
            Rule::BoundSuffix => bound_suffix::check,
            Rule::ClaimIndexSuffix => claim_index_suffix::check,
            Rule::WriteIdentitySuffix => write_identity_suffix::check,
            Rule::IdentityNotKey => identity_not_key::check,
            Rule::IntegerIdentifierNotKey => integer_identifier_not_key::check,
            Rule::IdentifierArrayNotKeys => identifier_array_not_keys::check,
            Rule::ForeignKeyColumnName => foreign_key_column_name::check,
            Rule::BooleanNotPredicate => boolean_not_predicate::check,
            Rule::TriggerName => trigger_name::check,
            Rule::ConstraintOnSealedTable => constraint_on_sealed_table::check,
        };
        check(context)
    }

    /// What every rule finds in `context`, rule by rule.
    #[must_use]
    pub fn check_all(context: &RuleContext<'_>) -> Vec<RuleFinding> {
        Rule::iter().flat_map(|rule| rule.check(context)).collect()
    }
}

/// What the rules read.
#[derive(Debug, Clone, Copy)]
pub struct RuleContext<'input> {
    /// The schema the migrations declare.
    schema: &'input DeclaredSchema,
    /// The formatter's vocabulary.
    vocabulary: &'input FormatterVocabulary,
    /// The releases sealed so far.
    ledger: &'input SealLedger,
}

impl<'input> RuleContext<'input> {
    /// The rules' view of `schema`, `vocabulary` and `ledger`.
    #[must_use]
    pub const fn new(schema: &'input DeclaredSchema, vocabulary: &'input FormatterVocabulary, ledger: &'input SealLedger) -> RuleContext<'input> {
        RuleContext { schema, vocabulary, ledger }
    }

    /// The schema the migrations declare.
    #[must_use]
    pub const fn schema(&self) -> &'input DeclaredSchema {
        self.schema
    }

    /// The formatter's vocabulary.
    #[must_use]
    pub const fn vocabulary(&self) -> &'input FormatterVocabulary {
        self.vocabulary
    }

    /// The releases sealed so far.
    #[must_use]
    pub const fn ledger(&self) -> &'input SealLedger {
        self.ledger
    }

    /// A finding of `rule` over the `length` bytes at `origin`, saying what
    /// `violation` renders as.
    #[must_use]
    pub fn finding(&self, rule: Rule, origin: Origin, length: usize, violation: &impl fmt::Display) -> RuleFinding {
        let range = self.schema.migration(origin.version()).map_or_else(
            || SourceRange::of_bytes("", 0..0),
            |migration| migration.range(origin.offset()..origin.offset() + length),
        );
        RuleFinding {
            source: SourceKey::Migration(origin.version()),
            finding: lint_finding(rule, range, violation.to_string()),
        }
    }

    /// A finding of `rule` over `bytes` of the vocabulary file `file`,
    /// saying what `violation` renders as.
    #[must_use]
    pub fn vocabulary_finding(&self, rule: Rule, file: VocabularyFile, bytes: Range<usize>, violation: &impl fmt::Display) -> RuleFinding {
        let list = match file {
            VocabularyFile::Functions => self.vocabulary.functions(),
            VocabularyFile::Keywords => self.vocabulary.keywords(),
        };
        RuleFinding {
            source: SourceKey::Vocabulary(file),
            finding: lint_finding(rule, SourceRange::of_bytes(list.text(), bytes), violation.to_string()),
        }
    }
}

/// The finding of `rule` at `range` saying `rendered`.
fn lint_finding(rule: Rule, range: SourceRange, rendered: String) -> LintFinding {
    LintFinding::new(
        LintRule::Andromeda(rule),
        LintSeverity::Warning,
        range,
        rendered,
        rule.get_message().map(str::to_owned),
        None,
    )
}

/// One rule a file breaks, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleFinding {
    /// The file.
    source: SourceKey,
    /// The finding.
    finding: LintFinding,
}

impl RuleFinding {
    /// The file.
    #[must_use]
    pub const fn source(&self) -> SourceKey {
        self.source
    }

    /// The finding.
    #[must_use]
    pub const fn finding(&self) -> &LintFinding {
        &self.finding
    }

    /// The finding, taken out.
    #[must_use]
    pub fn into_finding(self) -> LintFinding {
        self.finding
    }
}
