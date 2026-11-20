//! The SQL statements a PL/pgSQL routine embeds, read by the SQL parser.
//!
//! The PL/pgSQL parser hands every SQL expression of a routine back as text,
//! with the grammar it is written in. A statement planned as written is parsed
//! and walked like a top-level statement. A dynamic command is text until it
//! runs, and only its template says what it names.

use crate::{
    declared_schema::syntax_walk::SyntaxSites,
    plpgsql_tree::{PlpgsqlRoutine, SqlRole},
};
use pg_query::ParserOptions;

/// The walked syntax tree of every statement of `text`, parsed with `options`.
///
/// # Errors
///
/// Returns [`EmbeddedStatementError`] when `text` does not parse.
pub fn sites_of(text: &str, options: ParserOptions) -> Result<Vec<SyntaxSites>, EmbeddedStatementError> {
    pg_query::parse(text, options)
        .map_err(EmbeddedStatementError::Unparsed)?
        .protobuf
        .stmts
        .iter()
        .filter_map(|statement| statement.stmt.as_deref())
        .map(|statement| SyntaxSites::of(statement).map_err(EmbeddedStatementError::Unwalkable))
        .collect()
}

/// The walked syntax tree of every statement `routines` plan as written. A
/// statement that does not parse is left out: `function-body-bound` reports
/// it.
#[must_use]
pub fn static_sites(routines: &[PlpgsqlRoutine]) -> Vec<SyntaxSites> {
    routines
        .iter()
        .flat_map(PlpgsqlRoutine::embedded_sql)
        .filter(|sql| sql.role() == SqlRole::Static)
        .filter_map(|sql| sites_of(sql.query(), sql.parse_options()).ok())
        .flatten()
        .collect()
}

/// Why an embedded statement could not be read.
#[derive(Debug, thiserror::Error)]
pub enum EmbeddedStatementError {
    /// The statement does not parse.
    #[error("it does not parse: {0}")]
    Unparsed(pg_query::Error),
    /// The statement's tree could not be walked.
    #[error("its tree could not be walked: {0}")]
    Unwalkable(serde_json::Error),
}

#[cfg(test)]
mod tests {
    use crate::{
        declared_schema::embedded_statement::static_sites,
        plpgsql_tree::{PlpgsqlError, PlpgsqlRoutine},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_static_statement_is_walked_and_a_dynamic_command_and_an_unparsable_one_are_not() -> Result<(), PlpgsqlError> {
        let block = "do $$\nbegin\n    delete from staged where key = 1;\n    execute format('delete from %I.t', 's');\n    perform seal.key();\nend\n$$";
        let routines = PlpgsqlRoutine::parse(block, None)?;
        let sites = static_sites(&routines);
        let relations: Vec<&str> = sites
            .iter()
            .flat_map(|site| site.named_relations().map(|relation| relation.name().name()))
            .collect();
        let calls: Vec<&str> = sites.iter().flat_map(|site| site.calls().iter().map(|call| call.name().name())).collect();
        assert_eq!((relations, calls), (vec!["staged"], vec!["key"]));
        Ok(())
    }
}
