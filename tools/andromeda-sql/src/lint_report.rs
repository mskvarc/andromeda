//! The rendering of a finding as a diagnostic that quotes the migration and
//! underlines where the rule is broken.

use crate::{
    lint_finding::{LintFinding, LintSeverity},
    linted_source::LintedSource,
};
use miette::{GraphicalReportHandler, LabeledSpan, MietteDiagnostic, NamedSource, Report, Severity};
use std::{fmt, sync::Arc};

/// Renders findings against the text of the migration they are in.
#[derive(Debug)]
pub struct LintReport {
    /// The renderer every finding goes through.
    handler: GraphicalReportHandler,
}

impl LintReport {
    /// A report drawn by `handler`.
    #[must_use]
    pub const fn new(handler: GraphicalReportHandler) -> LintReport {
        LintReport { handler }
    }

    /// The diagnostic for `finding`, quoting `text`, the content of `file`,
    /// with the source named after the file's path from the repository root.
    ///
    /// # Errors
    ///
    /// Returns the formatting error the renderer reports.
    pub fn render(&self, file: &LintedSource, text: &Arc<str>, finding: &LintFinding) -> Result<String, fmt::Error> {
        let span = finding.range().byte_range(text);
        let mut diagnostic = MietteDiagnostic::new(finding.message())
            .with_severity(match finding.severity() {
                LintSeverity::Warning => Severity::Warning,
                LintSeverity::Error => Severity::Error,
            })
            .with_code(finding.rule().to_string())
            .with_label(LabeledSpan::new_with_span(None, span));
        if let Some(documentation) = finding.documentation() {
            diagnostic = diagnostic.with_url(documentation.as_str());
        }
        if let Some(help) = finding.help() {
            diagnostic = diagnostic.with_help(help);
        }
        // clone: the report owns its source; the clone shares the text
        let report = Report::new(diagnostic).with_source_code(NamedSource::new(file.to_string(), Arc::clone(text)));
        let mut rendered = String::new();
        self.handler.render_report(&mut rendered, report.as_ref())?;
        Ok(rendered)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        lint_finding::{LintFinding, LintRule, LintSeverity, SquawkRule},
        lint_report::LintReport,
        linted_source::LintedSource,
        migration_file::{MigrationFile, MigrationFileError},
        migration_set::MigrationSet,
        source_range::{LineColumn, SourceRange},
    };
    use miette::{GraphicalReportHandler, GraphicalTheme};
    use std::{fmt, path::PathBuf, sync::Arc};
    use url::Url;

    /// Why a rendering test could not set its finding up.
    #[derive(Debug, thiserror::Error)]
    enum Setup {
        /// The fixture file name breaks the naming rule.
        #[error(transparent)]
        File(#[from] MigrationFileError),
        /// The fixture documentation address does not parse.
        #[error(transparent)]
        Url(#[from] url::ParseError),
        /// The renderer failed.
        #[error(transparent)]
        Render(#[from] fmt::Error),
    }

    #[test]
    fn a_finding_quotes_its_line_under_the_file_path_and_position() -> Result<(), Setup> {
        let file = LintedSource::Migration(MigrationFile::new(
            MigrationSet::Tenant,
            PathBuf::from("/root/migrations/tenant/0002_entities.sql"),
        )?);
        let text: Arc<str> = Arc::from("create table entities (\n    kind_key integer not null\n);\n");
        let finding = LintFinding::new(
            LintRule::Squawk(SquawkRule::new("prefer-bigint-over-int")),
            LintSeverity::Warning,
            SourceRange::new(LineColumn::new(1, 13), LineColumn::new(1, 20)),
            "Using 32-bit integer fields can result in hitting the max `int` limit.".to_owned(),
            Some("Use 64-bit integer values instead to prevent hitting this limit.".to_owned()),
            Some(Url::parse("https://squawkhq.com/docs/prefer-bigint-over-int")?),
        );
        let rendered =
            LintReport::new(GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor()).with_links(false)).render(&file, &text, &finding)?;
        for expected in [
            "prefer-bigint-over-int",
            "[migrations/tenant/0002_entities.sql:2:14]",
            "kind_key integer not null",
            "help: Use 64-bit integer values",
            "(https://squawkhq.com/docs/prefer-bigint-over-int)",
        ] {
            assert!(rendered.contains(expected), "{expected:?} missing from:\n{rendered}");
        }
        Ok(())
    }
}
