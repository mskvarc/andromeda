//! The commented configuration `configuration generate` writes.

use crate::reference::{
    ReferenceError,
    key_listing::{ReferenceEntry, key_listing},
};

/// The opening comment of the template.
const PREAMBLE: &str = "\
# Andromeda configuration.
#
# Every key is shown at its built-in default. A key that is not set by
# default is shown commented out, at an example value, and the comment above
# it says what leaving it out means.";

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
                let paragraphs: Vec<&str> = key
                    .description
                    .as_deref()
                    .into_iter()
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
                .chain(textwrap::wrap(paragraph, COMMENT_WIDTH - 2).into_iter().map(|line| format!("# {line}")))
        })
        .collect()
}
