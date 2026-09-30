//! Comparing two robot runs by official test identifier: which cases newly pass,
//! which newly fail, which only one run executed, and the pass totals on each side.
//! A non-empty newly-failing list is the run's failure signal.

use crate::error::XtaskError;
use clap::{Args, ValueEnum};
use roxmltree::{Document, Node};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// Arguments to `xtask etsi diff`.
#[derive(Args)]
pub(crate) struct DiffArgs {
    /// `output.xml` of the run to compare against.
    #[arg(value_name = "BASELINE")]
    baseline: PathBuf,
    /// `output.xml` of the run under evaluation.
    #[arg(value_name = "CANDIDATE")]
    candidate: PathBuf,
    /// Report style.
    #[arg(long, value_enum, default_value_t = ReportStyle::Text)]
    format: ReportStyle,
}

impl DiffArgs {
    /// Parse both runs and report the verdict changes between them.
    ///
    /// # Errors
    ///
    /// Returns a missing- or unparseable-`output.xml` error, and
    /// [`XtaskError::NewlyFailing`] when a case newly fails.
    pub(crate) fn execute(self) -> Result<(), XtaskError> {
        let baseline = collect(&self.baseline)?;
        let candidate = collect(&self.candidate)?;
        let report = compare(&baseline, &candidate);
        print!("{}", report.render(&self.baseline, &baseline, &self.candidate, &candidate, self.format));
        if report.newly_failing.is_empty() {
            Ok(())
        } else {
            Err(XtaskError::NewlyFailing(report.newly_failing.len()))
        }
    }
}

/// The style a diff report is rendered in.
#[derive(Clone, Copy, ValueEnum)]
enum ReportStyle {
    /// Plain-text sections.
    Text,
    /// GitHub-flavoured markdown tables.
    Markdown,
}

/// The verdict a run recorded for a test case.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Verdict {
    /// The case passed.
    Pass,
    /// The case failed.
    Fail,
    /// Any other robot status, counted as neither a pass nor a fail.
    Other,
}

/// One executed test case and the verdict a run recorded for it.
struct TestOutcome {
    /// Full robot test name, official identifier first.
    name: String,
    /// The owning suite's name.
    suite: String,
    /// The recorded verdict.
    status: Verdict,
    /// The first line of the status message, empty when there is none.
    message: String,
}

/// The verdict changes between two runs, borrowing the outcomes from both maps.
struct Report<'runs> {
    /// Cases that failed in the baseline and pass in the candidate.
    newly_passing: Vec<&'runs TestOutcome>,
    /// Cases that passed in the baseline and fail in the candidate.
    newly_failing: Vec<&'runs TestOutcome>,
    /// Cases only the baseline run executed.
    only_baseline: Vec<&'runs TestOutcome>,
    /// Cases only the candidate run executed.
    only_candidate: Vec<&'runs TestOutcome>,
}

/// Read every executed test case out of one robot `output.xml`, keyed by official
/// identifier with the last occurrence winning, as the reference tool does.
fn collect(path: &Path) -> Result<BTreeMap<String, TestOutcome>, XtaskError> {
    if !path.is_file() {
        return Err(XtaskError::OutputXmlMissing(path.to_path_buf()));
    }
    let text = fs::read_to_string(path)?;
    collect_str(&text).map_err(|source| XtaskError::OutputXmlParse {
        path: path.to_path_buf(),
        source,
    })
}

/// Parse the outcomes out of the text of one `output.xml`, returning the XML
/// parse error on failure so the caller can attribute it to a path.
fn collect_str(text: &str) -> Result<BTreeMap<String, TestOutcome>, roxmltree::Error> {
    let document = Document::parse(text)?;
    let mut outcomes = BTreeMap::new();
    for test in document.descendants().filter(|node| node.has_tag_name("test")) {
        if let Some(outcome) = outcome_of(test) {
            outcomes.insert(official_identifier(&outcome.name).to_owned(), outcome);
        }
    }
    Ok(outcomes)
}

/// Build an outcome from a `test` node, or `None` when it carries no status element.
fn outcome_of(test: Node) -> Option<TestOutcome> {
    let name = test.attribute("name").unwrap_or_default().to_owned();
    let status = test.children().find(|child| child.has_tag_name("status"))?;
    let verdict = match status.attribute("status").unwrap_or_default() {
        "PASS" => Verdict::Pass,
        "FAIL" => Verdict::Fail,
        _status => Verdict::Other,
    };
    let message = status.text().unwrap_or_default().trim().lines().next().unwrap_or_default().to_owned();
    Some(TestOutcome {
        name,
        suite: nearest_suite_name(test),
        status: verdict,
        message,
    })
}

/// The name of the nearest ancestor suite of a test node, empty when none.
fn nearest_suite_name(test: Node) -> String {
    test.ancestors()
        .find(|node| node.has_tag_name("suite"))
        .and_then(|suite| suite.attribute("name"))
        .unwrap_or_default()
        .to_owned()
}

/// The leading official-identifier token of a robot test name.
fn official_identifier(name: &str) -> &str {
    name.split_once(' ').map_or(name, |(identifier, _rest)| identifier)
}

/// Classify the verdict changes between two runs. Both maps iterate in sorted key
/// order, so every section is ordered by official identifier.
fn compare<'runs>(baseline: &'runs BTreeMap<String, TestOutcome>, candidate: &'runs BTreeMap<String, TestOutcome>) -> Report<'runs> {
    let mut newly_passing = Vec::new();
    let mut newly_failing = Vec::new();
    for (key, candidate_outcome) in candidate {
        if let Some(baseline_outcome) = baseline.get(key) {
            match (baseline_outcome.status, candidate_outcome.status) {
                (Verdict::Fail, Verdict::Pass) => newly_passing.push(candidate_outcome),
                (Verdict::Pass, Verdict::Fail) => newly_failing.push(candidate_outcome),
                (Verdict::Pass | Verdict::Fail | Verdict::Other, Verdict::Pass | Verdict::Fail | Verdict::Other) => {}
            }
        }
    }
    let only_baseline = baseline
        .iter()
        .filter(|(key, _)| !candidate.contains_key(*key))
        .map(|(_, outcome)| outcome)
        .collect();
    let only_candidate = candidate
        .iter()
        .filter(|(key, _)| !baseline.contains_key(*key))
        .map(|(_, outcome)| outcome)
        .collect();
    Report {
        newly_passing,
        newly_failing,
        only_baseline,
        only_candidate,
    }
}

/// The number of passing cases in a run.
fn passing(outcomes: &BTreeMap<String, TestOutcome>) -> usize {
    outcomes.values().filter(|outcome| outcome.status == Verdict::Pass).count()
}

impl Report<'_> {
    /// Render the full report in the requested style, header first.
    fn render(
        &self,
        baseline_path: &Path,
        baseline: &BTreeMap<String, TestOutcome>,
        candidate_path: &Path,
        candidate: &BTreeMap<String, TestOutcome>,
        style: ReportStyle,
    ) -> String {
        let header = format!(
            "baseline  {}: {} pass / {} executed\ncandidate {}: {} pass / {} executed",
            baseline_path.display(),
            passing(baseline),
            baseline.len(),
            candidate_path.display(),
            passing(candidate),
            candidate.len(),
        );
        let mut lines = vec![
            match style {
                ReportStyle::Text => header,
                ReportStyle::Markdown => format!("```\n{header}\n```"),
            },
            String::new(),
        ];
        lines.extend(render_section("Newly passing", &self.newly_passing, style));
        lines.extend(render_section("Newly failing", &self.newly_failing, style));
        lines.extend(render_section("Executed only in the baseline run", &self.only_baseline, style));
        lines.extend(render_section("Executed only in the candidate run", &self.only_candidate, style));
        let mut rendered = lines.join("\n");
        rendered.push('\n');
        rendered
    }
}

/// Render one section of the report in the requested style.
fn render_section(title: &str, rows: &[&TestOutcome], style: ReportStyle) -> Vec<String> {
    match style {
        ReportStyle::Markdown => {
            let mut lines = vec![format!("### {title} ({})", rows.len()), String::new()];
            if rows.is_empty() {
                lines.push("None.".to_owned());
                lines.push(String::new());
                return lines;
            }
            lines.push("| Official test ID | Suite | Detail |".to_owned());
            lines.push("|---|---|---|".to_owned());
            lines.extend(
                rows.iter()
                    .map(|row| format!("| {} | {} | {} |", row.name, row.suite, row.message.replace('|', "\\|"))),
            );
            lines.push(String::new());
            lines
        }
        ReportStyle::Text => {
            let mut lines = vec![format!("{title} ({})", rows.len()), "-".repeat(title.len())];
            lines.extend(rows.iter().map(|row| format!("  {}  [{}]  {}", row.name, row.suite, row.message)));
            lines.push(String::new());
            lines
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::XtaskError,
        etsi::diff::{Verdict, collect, collect_str, compare, official_identifier},
    };
    use std::{env, fmt::Write, fs, process};

    /// One robot `output.xml` with a single suite and the given `(name, status)`
    /// tests, enough to exercise the comparison.
    fn output_xml(tests: &[(&str, &str)]) -> String {
        let body = tests.iter().fold(String::new(), |mut body, (name, status)| {
            let _ = write!(body, "<test name=\"{name}\"><status status=\"{status}\"/></test>");
            body
        });
        format!("<robot><suite name=\"NGSI-LD\">{body}</suite></robot>")
    }

    #[test]
    fn the_official_identifier_is_the_leading_token() {
        assert_eq!(official_identifier("001_02_02 EmptyJson"), "001_02_02");
        assert_eq!(official_identifier("Nameless"), "Nameless");
    }

    #[test]
    fn a_status_becomes_the_recorded_verdict() {
        let outcomes = collect_str(&output_xml(&[("001_02_02 A", "PASS"), ("002_01 B", "FAIL")])).expect("valid xml");
        assert_eq!(outcomes.len(), 2);
        assert!(matches!(outcomes["001_02_02"].status, Verdict::Pass));
        assert!(matches!(outcomes["002_01"].status, Verdict::Fail));
        assert_eq!(outcomes["001_02_02"].suite, "NGSI-LD");
    }

    #[test]
    fn a_case_that_stops_passing_is_newly_failing() {
        let baseline = collect_str(&output_xml(&[("001 A", "PASS")])).expect("valid baseline");
        let candidate = collect_str(&output_xml(&[("001 A", "FAIL")])).expect("valid candidate");
        let report = compare(&baseline, &candidate);
        assert_eq!(report.newly_failing.len(), 1);
        assert_eq!(report.newly_failing[0].name, "001 A");
        assert!(report.newly_passing.is_empty());
    }

    #[test]
    fn a_case_that_starts_passing_is_newly_passing() {
        let baseline = collect_str(&output_xml(&[("001 A", "FAIL")])).expect("valid baseline");
        let candidate = collect_str(&output_xml(&[("001 A", "PASS")])).expect("valid candidate");
        let report = compare(&baseline, &candidate);
        assert_eq!(report.newly_passing.len(), 1);
        assert!(report.newly_failing.is_empty());
    }

    #[test]
    fn a_case_run_only_on_one_side_is_reported_as_such() {
        let baseline = collect_str(&output_xml(&[("001 A", "PASS"), ("002 B", "PASS")])).expect("valid baseline");
        let candidate = collect_str(&output_xml(&[("001 A", "PASS")])).expect("valid candidate");
        let report = compare(&baseline, &candidate);
        assert_eq!(report.only_baseline.len(), 1);
        assert_eq!(report.only_baseline[0].name, "002 B");
        assert!(report.only_candidate.is_empty());
    }

    #[test]
    fn an_unchanged_pass_is_neither_newly_passing_nor_failing() {
        let baseline = collect_str(&output_xml(&[("001 A", "PASS")])).expect("valid baseline");
        let candidate = collect_str(&output_xml(&[("001 A", "PASS")])).expect("valid candidate");
        let report = compare(&baseline, &candidate);
        assert!(report.newly_passing.is_empty());
        assert!(report.newly_failing.is_empty());
    }

    #[test]
    fn a_missing_output_xml_names_the_absent_path() {
        let path = env::temp_dir().join(format!("andromeda-xtask-diff-missing-{}.xml", process::id()));
        let error = collect(&path).err().expect("an absent output.xml is rejected");
        assert!(matches!(error, XtaskError::OutputXmlMissing(missing) if missing == path));
    }

    #[test]
    fn a_malformed_output_xml_carries_its_path_and_the_xml_parse_error() {
        let path = env::temp_dir().join(format!("andromeda-xtask-diff-malformed-{}.xml", process::id()));
        fs::write(&path, "<robot><suite>").expect("write the malformed output.xml");
        let error = collect(&path).err().expect("a malformed output.xml is rejected");
        fs::remove_file(&path).expect("clean up the malformed output.xml");
        assert!(matches!(error, XtaskError::OutputXmlParse { path: parsed, .. } if parsed == path));
    }
}
