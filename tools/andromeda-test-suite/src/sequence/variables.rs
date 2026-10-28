//! The values a test fills its `{{name}}` placeholders from: the stack's addresses, the namespace, and every value a
//! capture took from an answer.

use crate::sequence::queried::Queried;
use chrono::{SecondsFormat, Utc};
use std::collections::HashMap;
use uuid::Uuid;

/// Why a template cannot be filled.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TemplateError {
    /// The template names a variable no one set.
    #[error("the variable {0} is not set")]
    Unset(String),
    /// A placeholder is not closed.
    #[error("a placeholder in {0:?} is not closed")]
    Unclosed(String),
    /// The variable holds a value no template can place.
    #[error("the variable {0} holds {1}, which a template cannot place")]
    Unplaceable(String, String),
}

/// The named values of one test.
#[derive(Debug, Clone, Default)]
pub struct Variables {
    /// Every value by name.
    values: HashMap<String, Queried>,
}

impl Variables {
    /// Sets `name` to `value`, replacing what it held.
    pub fn set(&mut self, name: &str, value: Queried) {
        self.values.insert(name.to_owned(), value);
    }

    /// Sets `name` to the string `value`.
    pub fn set_text(&mut self, name: &str, value: impl Into<String>) {
        self.set(name, Queried::String(value.into()));
    }

    /// The value of `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Queried> {
        self.values.get(name)
    }

    /// `template` with each `{{name}}` replaced by the text of the value of `name`. `newUuid` is a fresh UUID and
    /// `newDate` the current instant, at each placement.
    ///
    /// # Errors
    ///
    /// Returns [`TemplateError`] when a placeholder names an unset variable or is not closed.
    pub fn fill(&self, template: &str) -> Result<String, TemplateError> {
        let mut filled = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(start) = rest.find("{{") {
            filled.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let end = after.find("}}").ok_or_else(|| TemplateError::Unclosed(template.to_owned()))?;
            let name = after[..end].trim();
            filled.push_str(&self.placed(name)?);
            rest = &after[end + 2..];
        }
        filled.push_str(rest);
        Ok(filled)
    }

    /// The text placed for `name`.
    fn placed(&self, name: &str) -> Result<String, TemplateError> {
        match name {
            "newUuid" => Ok(Uuid::new_v4().to_string()),
            "newDate" => Ok(Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true)),
            other => {
                let value = self.values.get(other).ok_or_else(|| TemplateError::Unset(other.to_owned()))?;
                value
                    .template_text()
                    .ok_or_else(|| TemplateError::Unplaceable(other.to_owned(), value.kind().to_owned()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sequence::{
        queried::Queried,
        variables::{TemplateError, Variables},
    };
    use googletest::prelude::*;

    #[gtest]
    fn every_placeholder_takes_its_value() -> Result<()> {
        let mut variables = Variables::default();
        variables.set_text("ns", "abc");
        variables.set("n", Queried::Integer(3));
        verify_that!(variables.fill("urn:{{ns}}-{{ ns }}:{{n}}")?, eq("urn:abc-abc:3"))
    }

    #[gtest]
    fn an_unset_variable_is_an_error() {
        expect_that!(Variables::default().fill("{{nope}}"), err(eq(&TemplateError::Unset("nope".to_owned()))));
    }

    #[gtest]
    fn text_without_placeholders_is_unchanged() -> Result<()> {
        verify_that!(Variables::default().fill("{\"a\": 1}")?, eq("{\"a\": 1}"))
    }
}
