//! Generated payloads: a body too large to write out or whose exact size is its subject, built when the request is
//! sent from a template and the parts it names, so its counts follow the stack's budgets and its identifiers the test's
//! namespace.
//!
//! A template names a part as `{{name}}` and the namespace as `{{ns}}`. A part repeats a text, draws a closed ring
//! of positions, fills a nested template, or pads a template with one character up to a length in octets; a template
//! or a padded template may be written as one JSON string. A count is a number or a variable of the test, either plus
//! or minus a number.

use crate::sequence::{placeholder_name::PlaceholderName, queried::Queried, variables::Variables};
use serde_json::Value as Json;
use std::{
    f64::consts::TAU,
    iter::{once, repeat_n},
};

/// The point every ring is drawn around, inside it.
const CENTRE: (f64, f64) = (14.55, 46.05);

/// The radius of every ring, in degrees.
const RADIUS: f64 = 0.05;

/// The fraction digits a ring's ordinates are rounded to.
const ORDINATE_DIGITS: usize = 9;

/// Why a payload cannot be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PayloadError {
    /// A template names a part the payload does not define.
    #[error("the payload names the part {0}, which it does not define")]
    UnknownPart(PlaceholderName),
    /// The placeholder opened at this byte of a template is not closed.
    #[error("the placeholder at byte {0} of a template is not closed")]
    Unclosed(usize),
    /// A count names a variable that is not set or does not hold a number.
    #[error("the count variable {0} is not set to a number")]
    CountVariable(PlaceholderName),
    /// A count falls below zero, or a ring below the two positions that close one.
    #[error("the count {0} cannot be drawn")]
    Count(i64),
}

/// How many times a part repeats or how long it is: a base plus an offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Count {
    /// What the count starts from.
    base: CountBase,
    /// What is added to the base, negative to subtract.
    offset: i64,
}

/// What a count starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CountBase {
    /// A number.
    Literal(u64),
    /// The number a variable of the test holds, such as a budget of the stack.
    Variable(String),
}

impl Count {
    /// The number `value`.
    #[must_use]
    pub const fn literal(value: u64) -> Count {
        Count {
            base: CountBase::Literal(value),
            offset: 0,
        }
    }

    /// The number the variable `name` holds.
    #[must_use]
    pub fn of(name: &str) -> Count {
        Count {
            base: CountBase::Variable(name.to_owned()),
            offset: 0,
        }
    }

    /// The count plus `offset`, negative to subtract.
    #[must_use]
    pub const fn plus(mut self, offset: i64) -> Count {
        self.offset = offset;
        self
    }

    /// The count's value.
    fn value(&self, variables: &Variables) -> Result<usize, PayloadError> {
        let base = match &self.base {
            CountBase::Literal(value) => i64::try_from(*value).map_err(|_| PayloadError::Count(i64::MAX))?,
            CountBase::Variable(name) => variables
                .get(name)
                .and_then(Queried::template_text)
                .and_then(|text| text.parse::<i64>().ok())
                .ok_or_else(|| PayloadError::CountVariable(PlaceholderName::new(name.as_str())))?,
        };
        let total = base.saturating_add(self.offset);
        usize::try_from(total).map_err(|_| PayloadError::Count(total))
    }
}

/// How a template's text is placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// As it is.
    Text,
    /// As one JSON string holding it.
    JsonString,
}

/// One part a payload's templates name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    /// A text repeated `count` times, joined by the separator when there is one.
    Repeat {
        /// The text.
        text: String,
        /// How many times.
        count: Count,
        /// What joins two repetitions.
        separator: Option<String>,
    },
    /// A closed ring of `count` positions: one fewer vertices on a circle, and the first again.
    Ring(Count),
    /// A nested template.
    Template {
        /// The template.
        template: String,
        /// How its text is placed.
        placement: Placement,
    },
    /// A template followed by one character up to a length in octets, for a value whose exact length is the subject
    /// however long the namespace is.
    Pad {
        /// The template.
        template: String,
        /// The character it is padded with.
        with: char,
        /// The length in octets.
        length: Count,
        /// How its text is placed.
        placement: Placement,
    },
}

impl Part {
    /// `text` repeated `count` times.
    #[must_use]
    pub fn repeat(text: &str, count: Count) -> Part {
        Part::Repeat {
            text: text.to_owned(),
            count,
            separator: None,
        }
    }

    /// `text` repeated `count` times, joined by `separator`.
    #[must_use]
    pub fn repeat_separated(text: &str, separator: &str, count: Count) -> Part {
        Part::Repeat {
            text: text.to_owned(),
            count,
            separator: Some(separator.to_owned()),
        }
    }

    /// A closed ring of `count` positions.
    #[must_use]
    pub const fn ring(count: Count) -> Part {
        Part::Ring(count)
    }

    /// The nested `template`, placed as it is.
    #[must_use]
    pub fn template(template: &str) -> Part {
        Part::Template {
            template: template.to_owned(),
            placement: Placement::Text,
        }
    }

    /// `template` padded with `with` up to `length` octets, placed as it is.
    #[must_use]
    pub fn pad(template: &str, with: char, length: Count) -> Part {
        Part::Pad {
            template: template.to_owned(),
            with,
            length,
            placement: Placement::Text,
        }
    }

    /// The template or padded template placed as one JSON string; a repeat or ring is left as it is.
    #[must_use]
    #[expect(clippy::missing_const_for_fn, reason = "a const fn cannot drop the part it is handed")]
    pub fn as_string(mut self) -> Part {
        match &mut self {
            Part::Template { placement, .. } | Part::Pad { placement, .. } => *placement = Placement::JsonString,
            Part::Repeat { .. } | Part::Ring(_) => {}
        }
        self
    }
}

/// A body built from a template and the parts it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payload {
    /// The template of the whole body.
    template: String,
    /// The parts, by name.
    parts: Vec<(String, Part)>,
}

impl Payload {
    /// A payload of `template`.
    #[must_use]
    pub fn new(template: &str) -> Payload {
        Payload {
            template: template.to_owned(),
            parts: Vec::new(),
        }
    }

    /// The payload with the part `name`.
    #[must_use]
    pub fn part(mut self, name: &str, part: Part) -> Payload {
        self.parts.push((name.to_owned(), part));
        self
    }

    /// The body, with `{{ns}}` the namespace `variables` hold and every count read from them.
    ///
    /// # Errors
    ///
    /// Returns [`PayloadError`] when a template names an undefined part or a count cannot be drawn.
    pub fn render(&self, variables: &Variables) -> Result<String, PayloadError> {
        self.fill(&self.template, variables)
    }

    /// `template` with each placeholder replaced by the namespace or its part.
    fn fill(&self, template: &str, variables: &Variables) -> Result<String, PayloadError> {
        let mut filled = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(start) = rest.find("{{") {
            filled.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let end = after.find("}}").ok_or(PayloadError::Unclosed(template.len() - rest.len() + start))?;
            let name = &after[..end];
            if name == "ns" {
                filled.push_str(&variables.get(name).and_then(Queried::template_text).unwrap_or_default());
            } else {
                let part = self
                    .parts
                    .iter()
                    .find(|(defined, _)| defined == name)
                    .map(|(_, part)| part)
                    .ok_or_else(|| PayloadError::UnknownPart(PlaceholderName::new(name)))?;
                filled.push_str(&self.render_part(part, variables)?);
            }
            rest = &after[end + 2..];
        }
        filled.push_str(rest);
        Ok(filled)
    }

    /// The text of `part`.
    fn render_part(&self, part: &Part, variables: &Variables) -> Result<String, PayloadError> {
        Ok(match part {
            Part::Repeat { text, count, separator } => {
                let times = count.value(variables)?;
                match separator {
                    Some(separator) => vec![text.as_str(); times].join(separator),
                    None => text.repeat(times),
                }
            }
            Part::Ring(count) => ring(count.value(variables)?)?,
            Part::Template { template, placement } => placed(self.fill(template, variables)?, *placement),
            Part::Pad {
                template,
                with,
                length,
                placement,
            } => {
                let mut text = self.fill(template, variables)?;
                let missing = length.value(variables)?.saturating_sub(text.len()) / with.len_utf8();
                text.extend(repeat_n(*with, missing));
                placed(text, *placement)
            }
        })
    }
}

/// `text` placed as `placement` says.
fn placed(text: String, placement: Placement) -> String {
    match placement {
        Placement::Text => text,
        Placement::JsonString => Json::String(text).to_string(),
    }
}

/// A closed `GeoJSON` ring of `positions` positions around [`CENTRE`].
fn ring(positions: usize) -> Result<String, PayloadError> {
    let vertices = positions
        .checked_sub(1)
        .filter(|vertices| *vertices > 0)
        .ok_or_else(|| PayloadError::Count(i64::try_from(positions).unwrap_or(i64::MAX)))?;
    let vertex = |k: usize| {
        #[expect(clippy::cast_precision_loss, reason = "ring sizes stay far below 2^52")]
        let angle = TAU * k as f64 / vertices as f64;
        format!("[{},{}]", ordinate(CENTRE.0 + RADIUS * angle.cos()), ordinate(CENTRE.1 + RADIUS * angle.sin()))
    };
    let body = (0..vertices).chain(once(0)).map(vertex).collect::<Vec<_>>().join(",");
    Ok(format!("[{body}]"))
}

/// `value` rounded to [`ORDINATE_DIGITS`] fraction digits and written in its shortest form, with at least one fraction
/// digit.
fn ordinate(value: f64) -> String {
    let rounded = format!("{value:.ORDINATE_DIGITS$}");
    let trimmed = rounded.trim_end_matches('0');
    match trimmed.strip_suffix('.') {
        Some(whole) => format!("{whole}.0"),
        None => trimmed.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use crate::sequence::{
        payload::{Count, Part, Payload, PayloadError},
        placeholder_name::PlaceholderName,
        variables::Variables,
    };
    use googletest::prelude::*;

    fn variables() -> Variables {
        let mut variables = Variables::default();
        variables.set_text("ns", "abc");
        variables.set_text("budget", "4");
        variables
    }

    #[gtest]
    fn a_repeat_follows_its_count_variable_and_offset() -> Result<()> {
        let payload = Payload::new("[{{zeros}}]").part("zeros", Part::repeat_separated("0", ",", Count::of("budget").plus(1)));
        verify_that!(payload.render(&variables())?, eq("[0,0,0,0,0]"))
    }

    #[gtest]
    fn a_repeat_without_separator_concatenates() -> Result<()> {
        let payload = Payload::new("{{spaces}}").part("spaces", Part::repeat(" ", Count::literal(3)));
        verify_that!(payload.render(&variables())?, eq("   "))
    }

    #[gtest]
    fn a_ring_is_closed_and_holds_the_count_of_positions() -> Result<()> {
        let payload = Payload::new("{{ring}}").part("ring", Part::ring(Count::literal(5)));
        let rendered = payload.render(&variables())?;
        let positions: Vec<Vec<f64>> = serde_json::from_str(&rendered)?;
        expect_that!(positions.len(), eq(5));
        expect_that!(positions.first(), eq(positions.last()));
        verify_that!(rendered, starts_with("[[14.6,46.05],[14.55,46.1],"))
    }

    #[gtest]
    fn a_pad_reaches_its_length_in_octets_whatever_the_namespace() -> Result<()> {
        let payload = Payload::new("{{id}}").part("id", Part::pad("urn:{{ns}}:", 'a', Count::literal(12)).as_string());
        verify_that!(payload.render(&variables())?, eq("\"urn:abc:aaaa\""))
    }

    #[gtest]
    fn a_nested_template_can_be_placed_as_a_json_string() -> Result<()> {
        let payload = Payload::new("{\"v\":{{inner}}}")
            .part("inner", Part::template("{\"n\":[{{zeros}}]}").as_string())
            .part("zeros", Part::repeat_separated("0", ",", Count::literal(2)));
        verify_that!(payload.render(&variables())?, eq("{\"v\":\"{\\\"n\\\":[0,0]}\"}"))
    }

    #[gtest]
    fn an_undefined_part_is_an_error() {
        expect_that!(
            Payload::new("{{missing}}").render(&variables()),
            err(eq(&PayloadError::UnknownPart(PlaceholderName::new("missing"))))
        );
    }

    #[gtest]
    fn a_count_variable_that_is_not_set_is_an_error() {
        let payload = Payload::new("{{x}}").part("x", Part::repeat("a", Count::of("nope")));
        expect_that!(
            payload.render(&variables()),
            err(eq(&PayloadError::CountVariable(PlaceholderName::new("nope"))))
        );
    }
}
