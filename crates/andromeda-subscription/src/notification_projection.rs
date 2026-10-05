//! The clause 5.2.14 `pick` and `omit` members as a payload writes them.
//!
//! Table 5.2.14.1-1 types both as an array of "valid attribute projection
//! language string[s] as per clause 4.21". Validation and term expansion read
//! them through [`written_projection`], so a payload is parsed by one rule
//! whichever of the two reaches it first.

use crate::subscription_validation_reason::SubscriptionValidationReason;
use andromeda_ngsi_ld_query::projection::grammar::ProjectionList;
use serde_json::Value;
use strum::{Display, EnumIter, IntoStaticStr};

/// One of the two clause 4.21 projection members of `NotificationParams`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumIter, IntoStaticStr)]
#[strum(serialize_all = "camelCase")]
pub enum ProjectionMember {
    /// `notification.pick`.
    Pick,
    /// `notification.omit`.
    Omit,
}

impl ProjectionMember {
    /// The member name as a payload spells it.
    #[must_use]
    pub fn key(self) -> &'static str {
        self.into()
    }
}

/// The projection lists `member` of a written `notification` holds, one per
/// string, or `None` when the member is absent or `null`.
///
/// # Errors
///
/// Returns [`SubscriptionValidationReason::ProjectionNotStrings`] when the
/// member is not an array of strings, and
/// [`SubscriptionValidationReason::ProjectionInvalid`] when a string is not a
/// clause 4.21 projection list.
pub(crate) fn written_projection(notification: &Value, member: ProjectionMember) -> Result<Option<Vec<ProjectionList>>, SubscriptionValidationReason> {
    match notification.get(member.key()) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(strings)) => strings
            .iter()
            .map(|string| match string {
                Value::String(text) => text
                    .parse::<ProjectionList>()
                    .map_err(|source| SubscriptionValidationReason::ProjectionInvalid { member, source }),
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => {
                    Err(SubscriptionValidationReason::ProjectionNotStrings { member })
                }
            })
            .collect::<Result<Vec<_>, _>>()
            .map(Some),
        Some(Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => Err(SubscriptionValidationReason::ProjectionNotStrings { member }),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        notification_projection::{ProjectionMember, written_projection},
        subscription_validation_reason::SubscriptionValidationReason,
    };
    use andromeda_ngsi_ld_query::{error::QueryError, projection::grammar::ProjectionList};
    use serde_json::json;

    #[test]
    fn each_string_parses_to_its_own_projection_list() {
        let lists = written_projection(&json!({"pick": ["id", "owner{name},speed"]}), ProjectionMember::Pick)
            .expect("valid projection")
            .expect("present");
        let rendered: Vec<String> = lists.iter().map(ProjectionList::to_string).collect();
        pretty_assertions::assert_eq!(rendered, ["id", "owner{name},speed"]);
    }

    #[test]
    fn an_absent_or_null_member_holds_no_projection() {
        assert!(written_projection(&json!({}), ProjectionMember::Omit).expect("valid").is_none());
        assert!(written_projection(&json!({"omit": null}), ProjectionMember::Omit).expect("valid").is_none());
    }

    /// Table 5.2.14.1-1: "a valid attribute projection language string as per
    /// clause 4.21".
    #[test]
    fn a_malformed_string_is_refused_naming_its_member() {
        let refused = written_projection(&json!({"omit": ["owner{"]}), ProjectionMember::Omit).expect_err("unbalanced brace");
        assert!(
            matches!(
                refused,
                SubscriptionValidationReason::ProjectionInvalid {
                    member: ProjectionMember::Omit,
                    source: QueryError::ProjectionUnbalancedBrace { .. }
                }
            ),
            "got {refused:?}"
        );
    }

    /// Only strings are clause 4.21 text; a term tree written by a client is
    /// not a shape the specification defines.
    #[test]
    fn a_member_that_is_not_an_array_of_strings_is_refused() {
        for written in [
            json!({"pick": "id"}),
            json!({"pick": [{"name": "id", "linked": []}]}),
            json!({"pick": [["id"]]}),
        ] {
            let refused = written_projection(&written, ProjectionMember::Pick).expect_err("not strings");
            assert!(
                matches!(
                    refused,
                    SubscriptionValidationReason::ProjectionNotStrings {
                        member: ProjectionMember::Pick
                    }
                ),
                "{written}: {refused:?}"
            );
        }
    }
}
