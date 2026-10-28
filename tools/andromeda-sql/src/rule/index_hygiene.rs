//! `index-hygiene`: no index keeps an operator class, a hash call or a key
//! list another index already serves.
//!
//! A `text_pattern_ops` class orders text byte by byte for anchored patterns,
//! which the `C` collation every text column carries already does. A built-in
//! hash function may change its values in a major release, which silently
//! invalidates every index keyed by it. Of two indexes on one table with the
//! same method and predicate, the one whose keys lead the other's is
//! redundant: the longer one serves every query it serves, at the cost of a
//! second index write per row, unless it enforces uniqueness.

use crate::{
    declared_schema::index::DeclaredIndex,
    rule::{Rule, RuleContext, RuleFinding},
};
use std::ptr;

/// The operator classes that order text byte by byte.
const PATTERN_OPERATOR_CLASSES: &[&str] = &["text_pattern_ops", "varchar_pattern_ops", "bpchar_pattern_ops"];

/// The start of the name of every built-in hash function.
const HASH_FUNCTION_PREFIX: &str = "hash";

/// What the rule finds.
#[derive(Debug, derive_more::Display)]
enum IndexViolation<'schema> {
    /// A key names a byte-order pattern operator class.
    #[display("{index} keys with {class}, which the C collation makes needless")]
    PatternOperatorClass {
        /// The index.
        index: &'schema str,
        /// The operator class.
        class: &'schema str,
    },
    /// A key or predicate calls a built-in hash function.
    #[display("{index} calls {function}, whose values a major release may change")]
    HashFunction {
        /// The index.
        index: &'schema str,
        /// The hash function.
        function: &'schema str,
    },
    /// The index's keys lead another index's.
    #[display("{index} is redundant: {covering} has the same method and predicate and leads with its keys")]
    Redundant {
        /// The redundant index.
        index: &'schema str,
        /// The index that serves it.
        covering: &'schema str,
    },
}

/// Every pattern operator class, hash call and redundant index.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    let indexes = context.schema().indexes();
    indexes
        .iter()
        .flat_map(|index| {
            let name = index.name();
            let classes = index
                .elements()
                .iter()
                .filter_map(|element| element.operator_class())
                .filter(|class| PATTERN_OPERATOR_CLASSES.contains(&class.name()))
                .map(move |class| IndexViolation::PatternOperatorClass {
                    index: name,
                    class: class.name(),
                });
            let hashes = index
                .called_functions()
                .iter()
                .filter(|function| function.name().starts_with(HASH_FUNCTION_PREFIX))
                .map(move |function| IndexViolation::HashFunction {
                    index: name,
                    function: function.name(),
                });
            let redundant = indexes
                .iter()
                .find(|other| !ptr::eq(*other, index) && is_served_by(index, other))
                .map(|covering| IndexViolation::Redundant {
                    index: name,
                    covering: covering.name(),
                });
            classes
                .chain(hashes)
                .chain(redundant)
                .map(|violation| context.finding(Rule::IndexHygiene, index.origin(), name.len(), &violation))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Whether `index` is redundant beside `other`: neither is unique, both are on
/// one table with one method and one predicate, and `index`'s keys lead
/// `other`'s, an equal key list counting against the later of the two.
fn is_served_by(index: &DeclaredIndex, other: &DeclaredIndex) -> bool {
    let keys = |index: &DeclaredIndex| index.elements().iter().map(|element| element.written().to_owned()).collect::<Vec<_>>();
    let (own, others) = (keys(index), keys(other));
    let leads = others.len() > own.len() || (others.len() == own.len() && other.origin() < index.origin());
    !index.is_unique()
        && !other.is_unique()
        && index.table() == other.table()
        && index.method() == other.method()
        && index.predicate() == other.predicate()
        && leads
        && others.starts_with(&own)
}

#[cfg(test)]
mod tests {
    use crate::rule::{
        Rule,
        rule_fixture::{FixtureError, findings},
    };
    use pretty_assertions::assert_eq;

    const FILE: &str = "branch/0001_streams.sql";

    const TABLE: &str = "create table streams (\n    stream_key bigint not null,\n    entity_key bigint not null,\n    label text collate \"C\"\n);\n\n";

    #[test]
    fn distinct_indexes_are_accepted() -> Result<(), FixtureError> {
        let sql = format!(
            "{TABLE}create index idx_streams_entity_key on streams (entity_key);\n\ncreate index idx_streams_stream_key on streams (stream_key, entity_key)\nwhere\n    label is not null;\n\ncreate unique index uq_streams_stream_key on streams (stream_key);\n"
        );
        assert_eq!(findings(Rule::IndexHygiene, &[(FILE, &sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_pattern_class_a_hash_call_and_a_redundant_prefix_are_refused() -> Result<(), FixtureError> {
        let sql = format!(
            "{TABLE}create index idx_streams_label on streams (label text_pattern_ops);\n\ncreate index idx_streams_label_hash on streams (hashtext(label));\n\ncreate index idx_streams_entity_key on streams (entity_key);\n\ncreate index idx_streams_entity_key_stream_key on streams (entity_key, stream_key);\n"
        );
        assert_eq!(
            findings(Rule::IndexHygiene, &[(FILE, &sql)])?,
            [
                "migrations/branch/0001_streams.sql:7:14",
                "migrations/branch/0001_streams.sql:9:14",
                "migrations/branch/0001_streams.sql:11:14"
            ]
        );
        Ok(())
    }
}
