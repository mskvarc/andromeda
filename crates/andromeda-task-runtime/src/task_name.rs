//! Validated task names used by registration, health and lifecycle reporting.

use compact_str::CompactString;
use derive_more::Display;
use thiserror::Error;

/// Stable kebab-case identifier for one supervised task.
#[derive(Clone, Debug, Display, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskName(CompactString);

impl TaskName {
    /// Borrow the validated task name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl TryFrom<&str> for TaskName {
    type Error = TaskNameError;

    fn try_from(value: &str) -> Result<TaskName, TaskNameError> {
        let valid = !value.is_empty()
            && value.bytes().enumerate().all(|(index, byte)| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || (byte == b'-' && index > 0 && value.as_bytes().get(index.saturating_sub(1)) != Some(&b'-'))
            })
            && !value.ends_with('-');
        if !valid {
            return Err(TaskNameError);
        }
        Ok(TaskName(CompactString::new(value)))
    }
}

/// A task name was empty or was not lower kebab-case ASCII.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("task names must be non-empty lower kebab-case ASCII")]
pub struct TaskNameError;

#[cfg(test)]
mod tests {
    use crate::task_name::TaskName;

    #[test]
    fn lower_kebab_case_name_is_accepted() {
        assert!(TaskName::try_from("entity-expiry").is_ok());
    }

    #[test]
    fn malformed_names_are_rejected() {
        for malformed in ["", "EntityExpiry", "entity_expiry", "-entity", "entity-", "entity--expiry"] {
            assert!(TaskName::try_from(malformed).is_err(), "{malformed} must be rejected");
        }
    }
}
