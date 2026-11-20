//! The spacing of a function call: its name directly before its parenthesis.
//!
//! `pg_format` writes a call of a function its dictionary knows as `name(` and
//! of any other as `name (`, and it never sees a function statement or a `do`
//! block, whose bodies keep the layout they were written with. The canonical
//! layout writes every call as `name(`, read from the parse rather than from a
//! dictionary. The parser locates every call of a statement and of a
//! SQL-standard body. The PL/pgSQL parser keeps no offset of the statements a
//! routine embeds, so in a PL/pgSQL body a name counts as a call when one of
//! the routine's own statements calls a function of that name, it is followed
//! by a parenthesis, and it neither follows `as`, which opens an alias's column
//! list, nor a `.`, which makes it part of a longer name.

use crate::{
    declared_schema::{
        do_block,
        embedded_statement::static_sites,
        function::{is_plpgsql, trigger_return},
        qualified_name::QualifiedName,
        syntax_walk::SyntaxSites,
    },
    plpgsql_tree::{PlpgsqlError, PlpgsqlRoutine},
    sql_statement::{SqlStatement, SqlStatementError},
};
use pg_query::{
    NodeEnum,
    protobuf::{KeywordKind, ScanToken, Token},
};
use std::{collections::BTreeSet, ops::Range};

/// `sql` with every function call written as `name(`.
///
/// # Errors
///
/// Returns [`CallSpacingError`] when `sql` or a routine body in it does not
/// scan or parse.
pub fn with_tight_calls(sql: &str) -> Result<String, CallSpacingError> {
    let tokens = pg_query::scan(sql).map_err(CallSpacingError::Unscanned)?.tokens;
    let mut gaps = BTreeSet::new();
    for statement in SqlStatement::parse_all(sql, &tokens).map_err(CallSpacingError::Unparsed)? {
        let Some(node) = statement.node() else {
            continue;
        };
        let sites = SyntaxSites::of(node).map_err(CallSpacingError::Unwalkable)?;
        gaps.extend(sites.calls().iter().filter_map(|call| {
            let start = call.offset()?;
            let index = tokens.binary_search_by_key(&Some(start), |token| offset(token.start)).ok()?;
            call_gap(sql, &tokens, index, |written| written == spelled(call.name()))
        }));
        let span = statement.span();
        let Some(routines) = plpgsql_routines(node, &sql[span.clone()], span.start)? else {
            continue;
        };
        let called = static_sites(&routines)
            .iter()
            .flat_map(|sites| sites.calls().iter().map(|call| spelled(call.name())))
            .collect::<BTreeSet<_>>();
        for body in routine_bodies(sql, &tokens, span) {
            let text = &sql[body.clone()];
            let body_tokens = pg_query::scan(text).map_err(CallSpacingError::Unscanned)?.tokens;
            gaps.extend(
                (0..body_tokens.len())
                    .filter(|index| !follows_alias_or_qualifier(&body_tokens, *index))
                    .filter_map(|index| call_gap(text, &body_tokens, index, |written| called.contains(written)))
                    .map(|(start, end)| (start + body.start, end + body.start)),
            );
        }
    }
    Ok(without(sql, &gaps))
}

/// The PL/pgSQL routines of `node`, written as `statement`, which starts at
/// byte `start`; `None` for a statement that is no PL/pgSQL function or block.
fn plpgsql_routines(node: &NodeEnum, statement: &str, start: usize) -> Result<Option<Vec<PlpgsqlRoutine>>, CallSpacingError> {
    if let NodeEnum::CreateFunctionStmt(function) = node
        && is_plpgsql(function)
    {
        return Ok(Some(
            PlpgsqlRoutine::parse(statement, trigger_return(function, start)).map_err(CallSpacingError::Routine)?,
        ));
    }
    if let NodeEnum::DoStmt(block) = node
        && do_block::is_plpgsql(block)
    {
        return Ok(Some(PlpgsqlRoutine::parse(statement, None).map_err(CallSpacingError::Routine)?));
    }
    Ok(None)
}

/// The bytes of `sql` inside the delimiters of each dollar-quoted body within
/// `span`: the string after `as` in a function, and the string of a `do`
/// block.
fn routine_bodies(sql: &str, tokens: &[ScanToken], span: Range<usize>) -> Vec<Range<usize>> {
    let code = tokens
        .iter()
        .filter(|token| offset(token.start).is_some_and(|start| span.contains(&start)))
        .filter(|token| !matches!(Token::try_from(token.token), Ok(Token::SqlComment | Token::CComment)))
        .collect::<Vec<_>>();
    code.windows(2)
        .filter(|pair| matches!(Token::try_from(pair[0].token), Ok(Token::As | Token::Do | Token::Ident)))
        .map(|pair| pair[1])
        .filter(|token| Token::try_from(token.token) == Ok(Token::Sconst))
        .filter_map(|token| {
            let (start, end) = (offset(token.start)?, offset(token.end)?);
            let text = sql.get(start..end)?;
            let tag = text.strip_prefix('$')?.find('$')? + 2;
            (text.len() >= 2 * tag).then(|| start + tag..end - tag)
        })
        .collect()
}

/// The blank bytes between the name a call at `tokens[index]` spells and its
/// parenthesis, when `accepts` takes the name, written lowercase with its
/// parts joined by `.`, and the gap is spaces or tabs alone.
fn call_gap(text: &str, tokens: &[ScanToken], index: usize, accepts: impl Fn(&str) -> bool) -> Option<(usize, usize)> {
    let mut parts = vec![tokens.get(index).filter(|token| is_name(token))?];
    while let (Some(dot), Some(next)) = (tokens.get(index + 2 * parts.len() - 1), tokens.get(index + 2 * parts.len()))
        && Token::try_from(dot.token) == Ok(Token::Ascii46)
        && is_name(next)
    {
        parts.push(next);
    }
    let last = parts.last()?;
    let parenthesis = tokens
        .get(index + 2 * parts.len() - 1)
        .filter(|token| Token::try_from(token.token) == Ok(Token::Ascii40))?;
    let gap = (offset(last.end)?, offset(parenthesis.start)?);
    let blank = text.get(gap.0..gap.1)?;
    let written = parts
        .iter()
        .filter_map(|part| text.get(offset(part.start)?..offset(part.end)?))
        .collect::<Vec<_>>()
        .join(".")
        .to_ascii_lowercase();
    (!blank.is_empty() && blank.bytes().all(|byte| byte == b' ' || byte == b'\t') && accepts(&written)).then_some(gap)
}

/// Whether the token at `index` follows `as` or a `.`.
fn follows_alias_or_qualifier(tokens: &[ScanToken], index: usize) -> bool {
    index
        .checked_sub(1)
        .and_then(|previous| tokens.get(previous))
        .is_some_and(|previous| matches!(Token::try_from(previous.token), Ok(Token::As | Token::Ascii46)))
}

/// Whether `token` can name a function: an identifier or a keyword.
fn is_name(token: &ScanToken) -> bool {
    Token::try_from(token.token) == Ok(Token::Ident) || KeywordKind::try_from(token.keyword_kind).is_ok_and(|kind| kind != KeywordKind::NoKeyword)
}

/// `name` as a call writes it, lowercase, its schema joined by `.`.
fn spelled(name: &QualifiedName) -> String {
    name.schema()
        .map_or_else(|| name.name().to_owned(), |schema| format!("{schema}.{}", name.name()))
        .to_ascii_lowercase()
}

/// A scanner offset, which is never negative.
fn offset(position: i32) -> Option<usize> {
    usize::try_from(position).ok()
}

/// `sql` without the bytes of `gaps`.
fn without(sql: &str, gaps: &BTreeSet<(usize, usize)>) -> String {
    let mut kept = String::with_capacity(sql.len());
    let mut copied = 0;
    for (start, end) in gaps {
        kept.push_str(&sql[copied..*start]);
        copied = *end;
    }
    kept.push_str(&sql[copied..]);
    kept
}

/// Why the calls of a migration could not be read.
#[derive(Debug, thiserror::Error)]
pub enum CallSpacingError {
    /// The text or a routine body does not scan.
    #[error("the migration does not scan: {0}")]
    Unscanned(pg_query::Error),
    /// The text does not parse.
    #[error("the migration's statements could not be read: {0}")]
    Unparsed(SqlStatementError),
    /// A statement's tree could not be walked.
    #[error("a statement's tree could not be walked: {0}")]
    Unwalkable(serde_json::Error),
    /// A PL/pgSQL body does not parse.
    #[error(transparent)]
    Routine(PlpgsqlError),
}

#[cfg(test)]
mod tests {
    use crate::call_spacing::{CallSpacingError, with_tight_calls};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_statement_call_loses_its_space_and_a_keyword_list_keeps_it() -> Result<(), CallSpacingError> {
        assert_eq!(
            with_tight_calls("select f (1), s.g (2) from h () as x (a) where (a, b) overlaps (c, d) and exists (select);\n")?,
            "select f(1), s.g(2) from h() as x (a) where (a, b) overlaps (c, d) and exists (select);\n"
        );
        Ok(())
    }

    #[test]
    fn a_sql_standard_body_call_loses_its_space_and_the_function_name_keeps_it() -> Result<(), CallSpacingError> {
        assert_eq!(
            with_tight_calls(
                "create function s.f (a integer)\n    returns integer\n    language sql\n    immutable\n    parallel safe\n    return s.g (a);\n"
            )?,
            "create function s.f (a integer)\n    returns integer\n    language sql\n    immutable\n    parallel safe\n    return s.g(a);\n"
        );
        Ok(())
    }

    #[test]
    fn a_plpgsql_body_call_loses_its_space_and_an_alias_list_keeps_it() -> Result<(), CallSpacingError> {
        let block = "do $$\nbegin\n    perform s.g (1);\n    insert into t (a) select g from unnest(array[1]) as g (a);\nend\n$$;\n";
        assert_eq!(
            with_tight_calls(block)?,
            "do $$\nbegin\n    perform s.g(1);\n    insert into t (a) select g from unnest(array[1]) as g (a);\nend\n$$;\n"
        );
        Ok(())
    }

    #[test]
    fn a_call_split_over_lines_is_left_as_written() -> Result<(), CallSpacingError> {
        let sql = "select f\n    (1);\n";
        assert_eq!(with_tight_calls(sql)?, sql);
        Ok(())
    }
}
