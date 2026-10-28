//! Runs of consecutive code tokens a rule looks for, such as `create or
//! replace` or `security definer`.

use crate::declared_schema::parsed_migration::{ParsedMigration, SourceToken};
use pg_query::protobuf::Token;
use std::ops::Range;

/// The byte range of every run of code tokens of `migration`, comments
/// skipped, whose kinds are `kinds` in order.
#[must_use]
pub fn token_runs(migration: &ParsedMigration, kinds: &[Token]) -> Vec<Range<usize>> {
    let code = migration.tokens().iter().filter(|token| !token.is_comment()).collect::<Vec<&SourceToken>>();
    code.windows(kinds.len())
        .filter(|run| run.iter().zip(kinds).all(|(token, kind)| token.kind() == *kind))
        .filter_map(|run| Some(run.first()?.range().start..run.last()?.range().end))
        .collect()
}
