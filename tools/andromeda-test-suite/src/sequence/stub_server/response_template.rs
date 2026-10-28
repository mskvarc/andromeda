//! The response templates a stub may answer with: the `now` helper, which writes the current instant moved by an
//! offset, in a `java.text.SimpleDateFormat` pattern, as `unix` seconds, as `epoch` milliseconds or as RFC 3339; and an `each` block over an
//! inclusive `range`, whose body may name the index bare or zero-padded with `numberFormat` and skip a separator on
//! the first item with `unless @first`; and `randomValue`, a fresh UUID or a run of random letters and digits.
//!
//! The helpers let a test name instants relative to the moment the broker asks and answer large pages, as the
//! WireMock response templating does; every other helper is refused when the stub is set up.

use chrono::{DateTime, TimeDelta, Utc};
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

/// A `now` helper with its optional `offset`, `timezone` and `format` arguments, in either quote.
static NOW: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r#"\{\{now((?:\s+\w+=(?:'[^']*'|"[^"]*"))*)\s*\}\}?"#).ok());

/// An `each` block over an inclusive range, naming its index.
static EACH: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"(?s)\{\{#each \(range (\d+) (\d+)\) as \|(\w+)\|\}\}(.*?)\{\{/each\}\}").ok());

/// A separator written for every item but the first.
static UNLESS_FIRST: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"(?s)\{\{#unless @first\}\}(.*?)\{\{/unless\}\}").ok());

/// A `randomValue` helper: a fresh UUID, or random letters and digits of a length.
static RANDOM: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\{\{randomValue((?:\s+\w+=(?:'[^']*'|\d+))*)\s*\}\}").ok());

/// The `length` argument of a `randomValue` helper.
static LENGTH: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"length=(\d+)").ok());

/// One argument of a helper.
static ARGUMENT: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r#"(\w+)=(?:'([^']*)'|"([^"]*)")"#).ok());

/// Why a template cannot be rendered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TemplateError {
    /// The template uses a helper other than `now`.
    #[error("the response template {0:?} uses a helper the stub server does not render")]
    Unknown(String),
    /// An offset is not `<amount> <unit>`.
    #[error("{0:?} is not an offset")]
    Offset(String),
    /// A format pattern holds a letter the server does not write.
    #[error("{0:?} is not a date format the stub server writes")]
    Format(String),
}

/// `template` with each `now` helper replaced by the instant it names.
///
/// # Errors
///
/// Returns [`TemplateError`] when the template uses another helper or an argument is not understood.
pub fn render(template: &str) -> Result<String, TemplateError> {
    let (Some(now), Some(argument)) = (NOW.as_ref(), ARGUMENT.as_ref()) else {
        return Err(TemplateError::Unknown(template.to_owned()));
    };
    let expanded = randomised(&expand_each(template)?)?;
    let template = expanded.as_str();
    let instant = Utc::now();
    let mut rendered = String::with_capacity(template.len());
    let mut last = 0;
    for helper in now.captures_iter(template) {
        let Some(whole) = helper.get(0) else { continue };
        rendered.push_str(&template[last..whole.start()]);
        let mut moved = instant;
        let mut format = None;
        for found in argument.captures_iter(helper.get(1).map_or("", |arguments| arguments.as_str())) {
            let name = found.get(1).map_or("", |name| name.as_str());
            let value = found.get(2).or_else(|| found.get(3)).map_or("", |value| value.as_str());
            match name {
                "offset" => moved = offset(moved, value)?,
                "format" => format = Some(value.to_owned()),
                "timezone" => {}
                other => return Err(TemplateError::Unknown(other.to_owned())),
            }
        }
        rendered.push_str(&written(moved, format.as_deref())?);
        last = whole.end();
    }
    rendered.push_str(&template[last..]);
    if rendered.contains("{{") {
        return Err(TemplateError::Unknown(template.to_owned()));
    }
    Ok(rendered)
}

/// `template` with each `each` block over a range written out item by item.
fn expand_each(template: &str) -> Result<String, TemplateError> {
    let (Some(each), Some(unless_first)) = (EACH.as_ref(), UNLESS_FIRST.as_ref()) else {
        return Err(TemplateError::Unknown(template.to_owned()));
    };
    let mut expanded = String::with_capacity(template.len());
    let mut last = 0;
    for block in each.captures_iter(template) {
        let (Some(whole), Some(first), Some(end), Some(name), Some(body)) = (block.get(0), block.get(1), block.get(2), block.get(3), block.get(4)) else {
            continue;
        };
        let unknown = || TemplateError::Unknown(whole.as_str().to_owned());
        let first = first.as_str().parse::<u64>().map_err(|_| unknown())?;
        let end = end.as_str().parse::<u64>().map_err(|_| unknown())?;
        let name = name.as_str();
        let padded = Regex::new(&format!(r"\{{\{{numberFormat {name} '(0+)'\}}\}}")).map_err(|_| unknown())?;
        let bare = format!("{{{{{name}}}}}");
        expanded.push_str(&template[last..whole.start()]);
        for index in first..=end {
            let separated = unless_first.replace_all(body.as_str(), if index == first { "" } else { "$1" });
            let numbered = padded.replace_all(&separated, |found: &regex::Captures<'_>| {
                let width = found.get(1).map_or(0, |zeros| zeros.as_str().len());
                format!("{index:0width$}")
            });
            expanded.push_str(&numbered.replace(&bare, &index.to_string()));
        }
        last = whole.end();
    }
    expanded.push_str(&template[last..]);
    Ok(expanded)
}

/// `template` with each `randomValue` helper replaced by a fresh value: a UUID for `type='UUID'`, otherwise `length`
/// random lowercase letters and digits.
fn randomised(template: &str) -> Result<String, TemplateError> {
    let Some(random) = RANDOM.as_ref() else {
        return Err(TemplateError::Unknown(template.to_owned()));
    };
    let mut rendered = String::with_capacity(template.len());
    let mut last = 0;
    for helper in random.captures_iter(template) {
        let Some(whole) = helper.get(0) else { continue };
        rendered.push_str(&template[last..whole.start()]);
        let arguments = helper.get(1).map_or("", |arguments| arguments.as_str());
        if arguments.contains("UUID") {
            rendered.push_str(&Uuid::new_v4().to_string());
        } else {
            let length = LENGTH
                .as_ref()
                .and_then(|pattern| pattern.captures(arguments))
                .and_then(|found| found.get(1))
                .and_then(|digits| digits.as_str().parse::<usize>().ok())
                .ok_or_else(|| TemplateError::Unknown(whole.as_str().to_owned()))?;
            let letters = Uuid::new_v4().simple().to_string() + &Uuid::new_v4().simple().to_string();
            rendered.push_str(&letters.chars().cycle().take(length).collect::<String>());
        }
        last = whole.end();
    }
    rendered.push_str(&template[last..]);
    Ok(rendered)
}

/// `instant` moved by an offset such as `-15 minutes`.
fn offset(instant: DateTime<Utc>, offset: &str) -> Result<DateTime<Utc>, TemplateError> {
    let invalid = || TemplateError::Offset(offset.to_owned());
    let mut parts = offset.split_whitespace();
    let amount = parts.next().and_then(|amount| amount.parse::<i64>().ok()).ok_or_else(invalid)?;
    let delta = match parts.next().map(|unit| unit.trim_end_matches('s')) {
        Some("second") => TimeDelta::try_seconds(amount),
        Some("minute") => TimeDelta::try_minutes(amount),
        Some("hour") => TimeDelta::try_hours(amount),
        Some("day") => TimeDelta::try_days(amount),
        Some(_) | None => None,
    }
    .ok_or_else(invalid)?;
    instant.checked_add_signed(delta).ok_or_else(invalid)
}

/// `instant` in a `SimpleDateFormat` pattern, as `unix` seconds, or as RFC 3339 when no format is given.
fn written(instant: DateTime<Utc>, format: Option<&str>) -> Result<String, TemplateError> {
    match format {
        None => Ok(instant.to_rfc3339()),
        Some("unix") => Ok(instant.timestamp().to_string()),
        Some("epoch") => Ok(instant.timestamp_millis().to_string()),
        Some(pattern) => Ok(instant.format(&strftime(pattern)?).to_string()),
    }
}

/// The `strftime` pattern of a `SimpleDateFormat` pattern.
fn strftime(pattern: &str) -> Result<String, TemplateError> {
    let mut converted = String::new();
    let mut rest = pattern;
    while let Some(first) = rest.chars().next() {
        if first == '\'' {
            let after = &rest[1..];
            let end = after.find('\'').ok_or_else(|| TemplateError::Format(pattern.to_owned()))?;
            converted.push_str(&after[..end].replace('%', "%%"));
            rest = &after[end + 1..];
            continue;
        }
        let run = rest.chars().take_while(|letter| *letter == first).count();
        let token = &rest[..run];
        converted.push_str(match token {
            "yyyy" => "%Y",
            "MM" => "%m",
            "MMM" => "%b",
            "dd" => "%d",
            "HH" => "%H",
            "mm" => "%M",
            "ss" => "%S",
            "SSS" => "%3f",
            "EEE" => "%a",
            "z" => "GMT",
            other if !first.is_ascii_alphabetic() => other,
            _ => return Err(TemplateError::Format(pattern.to_owned())),
        });
        rest = &rest[run..];
    }
    Ok(converted)
}

#[cfg(test)]
mod tests {
    use crate::sequence::stub_server::response_template::{render, strftime};
    use googletest::prelude::*;

    #[gtest]
    fn a_simple_date_format_becomes_strftime() -> Result<()> {
        verify_that!(strftime("yyyy-MM-dd'T'HH:mm:ss'.000Z'")?, eq("%Y-%m-%dT%H:%M:%S.000Z"))
    }

    #[gtest]
    fn now_with_an_offset_renders_an_instant() -> Result<()> {
        let rendered = render(r#"{{now offset='240 seconds' format="yyyy-MM-dd'T'HH:mm:ss'Z'"}}"#)?;
        verify_that!(rendered.len(), eq(20))
    }

    #[gtest]
    fn an_each_block_over_a_range_writes_every_item() -> Result<()> {
        let rendered = render("[{{#each (range 0 2) as |n|}}{{#unless @first}},{{/unless}}\"{{numberFormat n '000'}}-{{n}}\"{{/each}}]")?;
        verify_that!(rendered, eq(r#"["000-0","001-1","002-2"]"#))
    }

    #[gtest]
    fn another_helper_is_refused() {
        expect_that!(render("{{jsonPath request.body '$.id'}}"), err(anything()));
    }
}
