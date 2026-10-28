//! One thing a linter found wrong with a migration.

use crate::{rule::Rule, source_range::SourceRange};
use url::Url;

/// The name of a Squawk rule, as Squawk spells it (`prefer-bigint-over-int`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display, derive_more::AsRef, serde::Deserialize)]
#[as_ref(str)]
#[serde(transparent)]
pub struct SquawkRule(String);

impl SquawkRule {
    /// The rule Squawk names `name`.
    #[must_use]
    pub fn new(name: impl Into<String>) -> SquawkRule {
        SquawkRule(name.into())
    }
}

/// The rule a finding breaks, owned by the linter that checks it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display)]
pub enum LintRule {
    /// A rule Squawk checks.
    #[display("{_0}")]
    Squawk(SquawkRule),
    /// A rule Andromeda checks on the parsed migrations.
    #[display("{_0}")]
    Andromeda(Rule),
}

/// How serious a finding is, as the linter grades it.
///
/// Every finding fails a lint run; the grade tells a rule the migration breaks
/// (a warning) from text the linter could not read at all (an error).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Deserialize)]
pub enum LintSeverity {
    /// The migration breaks a rule.
    Warning,
    /// The linter could not read the migration.
    Error,
}

/// One rule a migration breaks, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintFinding {
    /// The rule broken.
    rule: LintRule,
    /// How serious the finding is.
    severity: LintSeverity,
    /// Where in the file the rule is broken.
    range: SourceRange,
    /// What is wrong, in the linter's words.
    message: String,
    /// How to put it right, when the linter says.
    help: Option<String>,
    /// The page that explains the rule, when the linter publishes one.
    documentation: Option<Url>,
}

impl LintFinding {
    /// A finding of `rule` at `range`.
    #[must_use]
    pub const fn new(
        rule: LintRule,
        severity: LintSeverity,
        range: SourceRange,
        message: String,
        help: Option<String>,
        documentation: Option<Url>,
    ) -> LintFinding {
        LintFinding {
            rule,
            severity,
            range,
            message,
            help,
            documentation,
        }
    }

    /// The rule broken.
    #[must_use]
    pub const fn rule(&self) -> &LintRule {
        &self.rule
    }

    /// How serious the finding is.
    #[must_use]
    pub const fn severity(&self) -> LintSeverity {
        self.severity
    }

    /// Where in the file the rule is broken.
    #[must_use]
    pub const fn range(&self) -> SourceRange {
        self.range
    }

    /// What is wrong, in the linter's words.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// How to put it right, when the linter says.
    #[must_use]
    pub fn help(&self) -> Option<&str> {
        self.help.as_deref()
    }

    /// The page that explains the rule, when the linter publishes one.
    #[must_use]
    pub const fn documentation(&self) -> Option<&Url> {
        self.documentation.as_ref()
    }
}
