//! `comment-style`: a migration opens with its header template, and its
//! comments stay true for as long as the file exists.
//!
//! A released migration is frozen, so a comment in it must not cite anything
//! that can move: code, a document, a working record or another migration's
//! number. The header is a rule line, `-- NNNN: Title` naming the file's own
//! version (the one comment that may carry a migration number), a rule line,
//! comment lines, a closing rule line, and a `Spec:` line of the fixed form if
//! the migration cites the specification. Every comment line fits 80 columns,
//! uses no ` -- ` dash, carries no `TODO` or `FIXME`, and switches no linter
//! off: a rule that needs an exception changes the rule.

use crate::{
    declared_schema::parsed_migration::ParsedMigration,
    migration_version::MigrationVersion,
    rule::{Rule, RuleContext, RuleFinding},
};
use pg_query::protobuf::Token;

/// The rule line that opens and closes every migration header.
const RULE_LINE: &str = "-- ============================================================================";

/// The start every `Spec:` header line has.
const SPEC_LINE_PREFIX: &str = "-- Spec: ETSI GS CIM 009 v1.9.1 clause";

/// The start of a header line citing the specification.
const SPEC_LINE_START: &str = "-- Spec:";

/// The widest a comment line may be, in characters.
const MAXIMUM_COMMENT_WIDTH: usize = 80;

/// Text that cites code, a document or a working record.
const FORBIDDEN_FRAGMENTS: &[&str] = &["::", ".rs", "crates/", "andromeda-", "docs/", "Wave"];

/// The prose of a citation of a migration by number.
const MIGRATION_CITATION: &str = "migration 0";

/// Markers of unfinished work.
const UNFINISHED_MARKERS: &[&str] = &["TODO", "FIXME"];

/// The comment that switches Squawk off for a statement.
const SQUAWK_SUPPRESSION: &str = "squawk-ignore";

/// The line, counted from zero, that holds the title naming the version.
const TITLE_LINE: usize = 1;

/// What the rule finds.
#[derive(Debug, derive_more::Display)]
enum CommentViolation<'schema> {
    /// The header does not have the template's shape.
    #[display("the header is not a rule line, `-- {_0}: Title`, a rule line, comment lines and a closing rule line")]
    HeaderShape(MigrationVersion),
    /// A comment cites something that can move.
    #[display("the comment cites code, a document, a working record or another migration: {_0}")]
    ForbiddenReference(&'schema str),
    /// A comment line is too wide.
    #[display("the comment line is {_0} columns wide, past {MAXIMUM_COMMENT_WIDTH}")]
    TooWide(usize),
    /// A comment uses a double hyphen as a dash.
    #[display("the comment uses ` -- ` as a dash")]
    DoubleHyphenDash,
    /// A comment marks unfinished work.
    #[display("the comment marks unfinished work with {_0}")]
    Unfinished(&'schema str),
    /// A comment switches Squawk off.
    #[display("the comment switches a Squawk rule off; change the rule instead")]
    Suppression,
}

/// Every header that breaks the template, and every comment line that breaks
/// a prose rule.
#[must_use]
pub fn check(context: &RuleContext<'_>) -> Vec<RuleFinding> {
    context
        .schema()
        .migrations()
        .iter()
        .flat_map(|migration| {
            let text = migration.text();
            let header = (!header_is_well_formed(migration.file().version(), text)).then(|| {
                context.finding(
                    Rule::CommentStyle,
                    migration.origin(0),
                    text.lines().next().map_or(0, str::len),
                    &CommentViolation::HeaderShape(migration.file().version()),
                )
            });
            header.into_iter().chain(
                comment_violations(migration)
                    .into_iter()
                    .map(|(offset, length, violation)| context.finding(Rule::CommentStyle, migration.origin(offset), length, &violation)),
            )
        })
        .collect()
}

/// Whether `text`, the migration at `version`, opens with the template.
fn header_is_well_formed(version: MigrationVersion, text: &str) -> bool {
    let lines = text.lines().collect::<Vec<_>>();
    let title_prefix = format!("-- {version}: ");
    let title_is_well_formed = lines
        .get(TITLE_LINE)
        .and_then(|line| line.strip_prefix(title_prefix.as_str()))
        .is_some_and(|title| !title.is_empty() && !title.ends_with('.'));
    let body = lines.iter().skip(3).take_while(|line| **line != RULE_LINE);
    let body_is_comment_only = body.clone().all(|line| line.starts_with("--"));
    let spec_is_well_formed = body
        .clone()
        .filter(|line| line.starts_with(SPEC_LINE_START))
        .all(|line| line.starts_with(SPEC_LINE_PREFIX));
    let header_is_closed = lines.iter().skip(3).any(|line| *line == RULE_LINE);
    lines.first() == Some(&RULE_LINE)
        && title_is_well_formed
        && lines.get(2) == Some(&RULE_LINE)
        && body_is_comment_only
        && spec_is_well_formed
        && header_is_closed
}

/// Every prose violation of a comment of `migration`: its offset, length and
/// what it is.
fn comment_violations(migration: &ParsedMigration) -> Vec<(usize, usize, CommentViolation<'_>)> {
    let text = migration.text();
    let mut violations = Vec::new();
    for token in migration.tokens().iter().filter(|token| token.is_comment()) {
        let range = token.range();
        let written = migration.token_text(token);
        let mut line_start = range.start;
        for line in written.split('\n') {
            let prose = comment_prose(token.kind(), line);
            let at = (line_start, line.len());
            let line_number = text[..line_start].matches('\n').count();
            let full_line_start = text[..line_start].rfind('\n').map_or(0, |newline| newline + 1);
            let full_line_end = text[line_start..].find('\n').map_or(text.len(), |length| line_start + length);
            let width = text[full_line_start..full_line_end].chars().count();
            if line_number != TITLE_LINE && cites_forbidden_reference(prose) {
                violations.push((at.0, at.1, CommentViolation::ForbiddenReference(prose)));
            }
            if width > MAXIMUM_COMMENT_WIDTH {
                violations.push((at.0, at.1, CommentViolation::TooWide(width)));
            }
            if prose.contains(" -- ") {
                violations.push((at.0, at.1, CommentViolation::DoubleHyphenDash));
            }
            if let Some(marker) = UNFINISHED_MARKERS
                .iter()
                .find(|marker| prose.split(|character: char| !character.is_ascii_alphanumeric()).any(|word| word == **marker))
            {
                violations.push((at.0, at.1, CommentViolation::Unfinished(marker)));
            }
            if prose.to_ascii_lowercase().contains(SQUAWK_SUPPRESSION) {
                violations.push((at.0, at.1, CommentViolation::Suppression));
            }
            line_start += line.len() + 1;
        }
    }
    violations
}

/// The prose of one line of a comment: the text after a line comment's `--`,
/// or a block comment's line without its delimiters.
fn comment_prose(kind: Token, line: &str) -> &str {
    let prose = if kind == Token::SqlComment {
        line.trim_start().strip_prefix("--").unwrap_or(line)
    } else {
        let line = line.trim_start();
        let line = line.strip_prefix("/*").unwrap_or(line);
        line.strip_suffix("*/").unwrap_or(line)
    };
    prose.strip_prefix(' ').unwrap_or(prose)
}

/// Whether `prose` contains a four-digit number with a leading zero, the shape
/// of a migration version.
fn cites_migration_number(prose: &str) -> bool {
    prose
        .split(|character: char| !character.is_ascii_alphanumeric())
        .any(|word| word.len() == 4 && word.starts_with('0') && word.bytes().all(|byte| byte.is_ascii_digit()))
}

/// Whether `prose` cites code, a document, a working record or another
/// migration.
fn cites_forbidden_reference(prose: &str) -> bool {
    FORBIDDEN_FRAGMENTS.iter().any(|fragment| prose.contains(fragment)) || cites_migration_number(prose) || prose.to_lowercase().contains(MIGRATION_CITATION)
}

#[cfg(test)]
mod tests {
    use crate::{
        migration_version::{MigrationVersion, MigrationVersionError},
        rule::{
            Rule,
            comment_style::{cites_forbidden_reference, header_is_well_formed},
            rule_fixture::{FixtureError, findings},
        },
    };
    use pretty_assertions::assert_eq;

    const RULE: &str = "-- ============================================================================";

    const FILE: &str = "tenant/0007_types.sql";

    fn migration(body: &str) -> String {
        format!("{RULE}\n-- 0007: Composite types\n{RULE}\n-- The composite types.\n{RULE}\n{body}")
    }

    #[test]
    fn a_migration_with_the_header_and_plain_comments_is_accepted() -> Result<(), FixtureError> {
        let sql = migration("-- A pair of values; see clause 4.5.2.\ncreate type pair as (\n    left_value integer\n);\n");
        assert_eq!(findings(Rule::CommentStyle, &[(FILE, &sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_header_naming_another_version_is_refused_at_its_first_line() -> Result<(), FixtureError> {
        let sql = migration("select 1;\n").replace("0007:", "0008:");
        assert_eq!(findings(Rule::CommentStyle, &[(FILE, &sql)])?, ["migrations/tenant/0007_types.sql:1:1"]);
        Ok(())
    }

    #[test]
    fn each_prose_rule_is_refused_at_its_comment() -> Result<(), FixtureError> {
        let wide = format!("-- {}", "w".repeat(78));
        let sql = migration(&format!(
            "-- As in 0006.\n{wide}\n-- A dash -- here.\n-- TODO tighten.\nselect 1; -- squawk-ignore ban-drop-column\n/* see docs/storage.md */\n"
        ));
        assert_eq!(
            findings(Rule::CommentStyle, &[(FILE, &sql)])?,
            [
                "migrations/tenant/0007_types.sql:6:1",
                "migrations/tenant/0007_types.sql:7:1",
                "migrations/tenant/0007_types.sql:8:1",
                "migrations/tenant/0007_types.sql:9:1",
                "migrations/tenant/0007_types.sql:10:11",
                "migrations/tenant/0007_types.sql:11:1",
            ]
        );
        Ok(())
    }

    #[test]
    fn a_double_hyphen_inside_a_string_constant_is_not_a_comment() -> Result<(), FixtureError> {
        let sql = migration("do $$\nbegin\n    raise notice 'requires preloading -- skipping';\nend\n$$;\n");
        assert_eq!(findings(Rule::CommentStyle, &[(FILE, &sql)])?, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn a_header_with_a_malformed_spec_line_or_no_closing_rule_is_refused() -> Result<(), MigrationVersionError> {
        let version = "0001".parse::<MigrationVersion>()?;
        let unpinned = format!("{RULE}\n-- 0001: Tenant registry\n{RULE}\n-- Spec: clause 6.3.14\n{RULE}\n");
        let unclosed = format!("{RULE}\n-- 0001: Tenant registry\n{RULE}\n-- Purpose.\n");
        assert!(!header_is_well_formed(version, &unpinned));
        assert!(!header_is_well_formed(version, &unclosed));
        Ok(())
    }

    #[test]
    fn citations_of_code_documents_records_and_migrations_are_detected() {
        assert!(cites_forbidden_reference("see `crates/andromeda-ngsi-ld-lexicon`"));
        assert!(cites_forbidden_reference("`queue::claimable_at`"));
        assert!(cites_forbidden_reference("the live-only 0029"));
        assert!(cites_forbidden_reference("added by migration 0008"));
        assert!(cites_forbidden_reference("measured in Wave 3"));
        assert!(!cites_forbidden_reference("clause 5.2.12 and Table 5.2.12-1"));
        assert!(!cites_forbidden_reference("RFC 7946 section 3.1.1"));
        assert!(!cites_forbidden_reference("a comparison beyond 2^53"));
    }
}
