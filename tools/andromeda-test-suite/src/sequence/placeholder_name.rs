//! The name a `{{name}}` placeholder of a test template names.

use std::fmt;

/// The name a placeholder names: a variable, a part of a payload, or a helper of a response template.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlaceholderName(String);

impl PlaceholderName {
    /// The placeholder name `name`, as the template writes it.
    #[must_use]
    pub fn new(name: impl Into<String>) -> PlaceholderName {
        PlaceholderName(name.into())
    }
}

// Hand-written: the name is shown as the template writes it.
impl fmt::Display for PlaceholderName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
