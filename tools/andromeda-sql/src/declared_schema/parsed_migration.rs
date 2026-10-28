//! One migration as the rules read it: its text, its statements, and every
//! token of it, the tokens of each routine body included.
//!
//! A function body or a `do` block is a string constant to the SQL scanner,
//! but its code is code: it is scanned again on its own, and its tokens are
//! placed in the stream right after the string constant that holds them, at
//! their offsets in the file.

use crate::{
    declared_schema::origin::Origin,
    migration_file::MigrationFile,
    source_range::SourceRange,
    sql_statement::{SqlStatement, SqlStatementError},
};
use pg_query::{
    NodeEnum,
    protobuf::{ScanToken, Token},
};
use std::{ops::Range, sync::Arc};

/// Where a token was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenPlace {
    /// In the migration's own statements.
    Statement,
    /// Inside the body of a function or a `do` block.
    RoutineBody,
}

/// One token of a migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceToken {
    /// What the scanner read the token as.
    kind: Token,
    /// The bytes of the file the token covers.
    range: Range<usize>,
    /// Where the token was found.
    place: TokenPlace,
}

impl SourceToken {
    /// What the scanner read the token as.
    #[must_use]
    pub const fn kind(&self) -> Token {
        self.kind
    }

    /// The bytes of the file the token covers.
    #[must_use]
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    /// Where the token was found.
    #[must_use]
    pub const fn place(&self) -> TokenPlace {
        self.place
    }

    /// Whether the token is a comment.
    #[must_use]
    pub const fn is_comment(&self) -> bool {
        matches!(self.kind, Token::SqlComment | Token::CComment)
    }
}

/// One migration, parsed and scanned.
#[derive(Debug)]
pub struct ParsedMigration {
    /// The file.
    file: MigrationFile,
    /// Its text.
    text: Arc<str>,
    /// Its top-level statements, in order.
    statements: Vec<SqlStatement>,
    /// Every token, routine bodies included, in file order.
    tokens: Vec<SourceToken>,
}

impl ParsedMigration {
    /// Parse and scan `text`, the content of `file`.
    ///
    /// # Errors
    ///
    /// Returns [`MigrationParseError`] when the text or a routine body does not
    /// parse or scan.
    pub fn parse(file: MigrationFile, text: Arc<str>) -> Result<ParsedMigration, MigrationParseError> {
        let scanned = pg_query::scan(&text).map_err(MigrationParseError::Scan)?.tokens;
        let statements = SqlStatement::parse_all(&text, &scanned)?;
        let bodies = statements
            .iter()
            .filter_map(|statement| body_token_starts(&scanned, statement))
            .flatten()
            .collect::<Vec<_>>();
        let mut tokens = Vec::with_capacity(scanned.len());
        for token in &scanned {
            let token = source_token(token, 0, TokenPlace::Statement)?;
            let range = token.range();
            tokens.push(token);
            if bodies.contains(&range.start) {
                let content = body_content(&text[range.clone()]);
                let content = range.start + content.start..range.start + content.end;
                for inner in pg_query::scan(&text[content.clone()]).map_err(MigrationParseError::Scan)?.tokens {
                    tokens.push(source_token(&inner, content.start, TokenPlace::RoutineBody)?);
                }
            }
        }
        Ok(ParsedMigration {
            file,
            text,
            statements,
            tokens,
        })
    }

    /// The file.
    #[must_use]
    pub const fn file(&self) -> &MigrationFile {
        &self.file
    }

    /// Its text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Its top-level statements, in order.
    #[must_use]
    pub fn statements(&self) -> &[SqlStatement] {
        &self.statements
    }

    /// Every token, routine bodies included, in file order.
    #[must_use]
    pub fn tokens(&self) -> &[SourceToken] {
        &self.tokens
    }

    /// The text of `token`.
    #[must_use]
    pub fn token_text(&self, token: &SourceToken) -> &str {
        &self.text[token.range()]
    }

    /// Where byte `offset` of the file is, for an object of the file.
    #[must_use]
    pub const fn origin(&self, offset: usize) -> Origin {
        Origin::new(self.file.version(), offset)
    }

    /// The line and column range of `bytes`.
    #[must_use]
    pub fn range(&self, bytes: Range<usize>) -> SourceRange {
        SourceRange::of_bytes(&self.text, bytes)
    }

    /// The statement tokens, comments left out, that lie within `span`.
    pub fn code_tokens(&self, span: Range<usize>) -> impl Iterator<Item = &SourceToken> {
        self.tokens
            .iter()
            .filter(move |token| token.place == TokenPlace::Statement && !token.is_comment() && span.contains(&token.range.start))
    }

    /// The offset of the first name token within `span` that spells `name`,
    /// quoted or not, keywords included because a name may be one.
    #[must_use]
    pub fn name_offset(&self, span: Range<usize>, name: &str) -> Option<usize> {
        self.code_tokens(span)
            .find(|token| {
                let written = self.token_text(token);
                written
                    .strip_prefix('"')
                    .and_then(|quoted| quoted.strip_suffix('"'))
                    .map_or_else(|| written.eq_ignore_ascii_case(name), |quoted| quoted == name)
            })
            .map(|token| token.range.start)
    }

    /// Where the list item starting at `offset` ends: before the comma or the
    /// closing parenthesis that follows it at its own nesting depth, or the
    /// end of its statement.
    #[must_use]
    pub fn item_end(&self, offset: usize, statement: Range<usize>) -> usize {
        let mut depth = 0_usize;
        for token in self.code_tokens(offset..statement.end) {
            let closes = token.kind == Token::Ascii41;
            if depth == 0 && (closes || matches!(token.kind, Token::Ascii44 | Token::Ascii59)) {
                return token.range.start;
            }
            if closes {
                depth -= 1;
            } else if token.kind == Token::Ascii40 {
                depth += 1;
            }
        }
        statement.end
    }
}

/// `token` of a text scanned at byte `base` of the file.
fn source_token(token: &ScanToken, base: usize, place: TokenPlace) -> Result<SourceToken, MigrationParseError> {
    let kind = Token::try_from(token.token).map_err(|_| MigrationParseError::UnknownToken(token.token))?;
    let start = usize::try_from(token.start).map_err(|_| MigrationParseError::UnknownToken(token.token))?;
    let end = usize::try_from(token.end).map_err(|_| MigrationParseError::UnknownToken(token.token))?;
    Ok(SourceToken {
        kind,
        range: base + start..base + end,
        place,
    })
}

/// The start of every string constant of `statement` that holds a routine
/// body: the one after `as` in a `create function`, and the one of a `do`.
fn body_token_starts(tokens: &[ScanToken], statement: &SqlStatement) -> Option<Vec<usize>> {
    let span = statement.span();
    let within = tokens
        .iter()
        .filter(|token| usize::try_from(token.start).is_ok_and(|start| span.contains(&start)))
        .filter(|token| !matches!(Token::try_from(token.token), Ok(Token::SqlComment | Token::CComment)));
    let starts = |token: &ScanToken| usize::try_from(token.start).ok();
    let node = statement.node()?;
    if matches!(node, NodeEnum::CreateFunctionStmt(_)) {
        let mut after_as = false;
        let mut bodies = Vec::new();
        for token in within {
            let kind = Token::try_from(token.token).ok();
            if after_as && kind == Some(Token::Sconst) {
                bodies.extend(starts(token));
            }
            after_as = kind == Some(Token::As);
        }
        Some(bodies)
    } else if matches!(node, NodeEnum::DoStmt(_)) {
        Some(
            within
                .filter(|token| Token::try_from(token.token).ok() == Some(Token::Sconst))
                .filter_map(starts)
                .take(1)
                .collect(),
        )
    } else {
        None
    }
}

/// The bytes of a string constant's text that hold its content: between the
/// dollar tags of a dollar-quoted string, or between the quotes of a quoted one.
fn body_content(constant: &str) -> Range<usize> {
    let (open, close) = match constant.strip_prefix('$').and_then(|rest| rest.find('$')) {
        Some(tag_end) => (tag_end + 2, tag_end + 2),
        None => (constant.find('\'').map_or(0, |quote| quote + 1), 1),
    };
    open.min(constant.len())..constant.len().saturating_sub(close).max(open.min(constant.len()))
}

/// Why a migration could not be parsed or scanned.
#[derive(Debug, thiserror::Error)]
pub enum MigrationParseError {
    /// The statements could not be read.
    #[error(transparent)]
    Statements(#[from] SqlStatementError),
    /// The text or a routine body could not be scanned.
    #[error("it could not be scanned: {0}")]
    Scan(pg_query::Error),
    /// The scanner reported a token kind or offset it does not define.
    #[error("the scanner reported an unknown token {0}")]
    UnknownToken(i32),
}

#[cfg(test)]
mod tests {
    use crate::{
        declared_schema::parsed_migration::{MigrationParseError, ParsedMigration, TokenPlace},
        migration_file::{MigrationFile, MigrationFileError},
        migration_set::MigrationSet,
    };
    use pg_query::protobuf::Token;
    use pretty_assertions::assert_eq;
    use std::{path::PathBuf, sync::Arc};

    /// Why a test could not set its migration up.
    #[derive(Debug, thiserror::Error)]
    enum Setup {
        /// The fixture name is not a migration name.
        #[error(transparent)]
        File(#[from] MigrationFileError),
        /// The fixture does not parse.
        #[error(transparent)]
        Parse(#[from] MigrationParseError),
    }

    fn parsed(sql: &str) -> Result<ParsedMigration, Setup> {
        let file = MigrationFile::new(MigrationSet::Tenant, PathBuf::from("/root/migrations/tenant/0001_fixture.sql"))?;
        Ok(ParsedMigration::parse(file, Arc::from(sql))?)
    }

    #[test]
    fn a_function_body_is_scanned_in_place() -> Result<(), Setup> {
        let migration = parsed("create function f () returns integer language plpgsql as $$\nbegin\n    return 1; -- one\nend\n$$;\n")?;
        let body: Vec<(&str, Token)> = migration
            .tokens()
            .iter()
            .filter(|token| token.place() == TokenPlace::RoutineBody)
            .map(|token| (migration.token_text(token), token.kind()))
            .collect();
        assert_eq!(
            body,
            [
                ("begin", Token::BeginP),
                ("return", Token::Return),
                ("1", Token::Iconst),
                (";", Token::Ascii59),
                ("-- one", Token::SqlComment),
                ("end", Token::EndP),
            ]
        );
        Ok(())
    }

    #[test]
    fn a_list_item_ends_before_the_comma_at_its_depth() -> Result<(), Setup> {
        let sql = "create table t (a numeric(10, 2) not null, b text);";
        let migration = parsed(sql)?;
        let start = sql.find("a numeric").unwrap_or_default();
        let end = migration.item_end(start, 0..sql.len());
        assert_eq!(&sql[start..end], "a numeric(10, 2) not null");
        Ok(())
    }
}
