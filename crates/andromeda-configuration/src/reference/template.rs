//! The commented configuration `configuration generate` writes.

use crate::reference::{
    ReferenceError,
    key_listing::{ReferenceEntry, key_listing},
    schema_node::ChoiceRange,
};
use textwrap::{Options, WordSeparator, WrapAlgorithm};

/// The opening comment of the template.
const PREAMBLE: &str = "\
# Andromeda configuration.
#
# Every key is shown at its built-in default. A key that is not set by
# default is shown commented out, at an example value, and the comment above
# it says what leaving it out means. A key with no default, such as
# database.connection, is shown commented out too: set it here or in the
# environment, or the broker does not start.";

/// The widest a comment line runs, its `# ` included.
const COMMENT_WIDTH: usize = 80;

/// The template: every table and key of the configuration, each under its
/// operator description, at its built-in default or, when it is not set by
/// default, commented out at an example value.
///
/// # Errors
///
/// Returns a [`ReferenceError`] when the configuration cannot be listed.
pub fn configuration_template() -> Result<String, ReferenceError> {
    let mut lines = vec![PREAMBLE.to_owned()];
    for entry in key_listing()? {
        lines.push(String::new());
        match entry {
            ReferenceEntry::Table(table) => {
                lines.extend(comment_lines(table.description.as_deref()));
                lines.push(table.header_line());
                lines.extend(table.member_lines());
            }
            ReferenceEntry::Key(key) => {
                let limits: Vec<String> = key.limits.iter().map(ToString::to_string).collect();
                let rules = (!limits.is_empty()).then(|| format!("Must be {}.", limits.join(", and ")));
                // A key that also takes a value of another shape names its
                // keywords in its own description.
                let values: Vec<&str> = match key.choice_range {
                    ChoiceRange::Closed => key.choices.iter().map(|choice| choice.value.as_str()).collect(),
                    ChoiceRange::Open => Vec::new(),
                };
                let choices = (!values.is_empty()).then(|| format!("One of {}.", values.join(", ")));
                let paragraphs: Vec<&str> = key
                    .description
                    .as_deref()
                    .into_iter()
                    .chain(choices.as_deref())
                    .chain(rules.as_deref())
                    .chain(key.conditions.iter().copied())
                    .collect();
                lines.extend(comment_lines(Some(&paragraphs.join("\n\n"))));
                lines.push(key.line());
            }
        }
    }
    lines.push(String::new());
    Ok(lines.join("\n"))
}

/// `description` as comment lines no wider than [`COMMENT_WIDTH`], an empty
/// comment line between its paragraphs. A paragraph ends at a blank line; a
/// single line break inside one reads as a space.
fn comment_lines(description: Option<&str>) -> Vec<String> {
    let paragraphs: Vec<String> = description
        .into_iter()
        .flat_map(|description| description.split("\n\n"))
        .map(|paragraph| paragraph.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    paragraphs
        .iter()
        .enumerate()
        .flat_map(|(index, paragraph)| {
            let separator = (index > 0).then(|| String::from("#"));
            separator
                .into_iter()
                .chain(textwrap::wrap(paragraph, comment_wrapping()).into_iter().map(|line| format!("# {line}")))
        })
        .collect()
}

/// How a comment paragraph is wrapped: at ASCII spaces only, filling each line in turn. Both are named rather than
/// left to `textwrap`'s defaults, which change with the features another crate of the build enables, so the generated
/// template is the same whichever packages one build compiles.
fn comment_wrapping() -> Options<'static> {
    Options::new(COMMENT_WIDTH - 2)
        .word_separator(WordSeparator::AsciiSpace)
        .wrap_algorithm(WrapAlgorithm::FirstFit)
}
