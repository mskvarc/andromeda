//! The members of a `ContextSourceRegistration` payload, by the names clause
//! 5.2.9 gives them.

use strum::{Display, IntoStaticStr};

/// A member of a `ContextSourceRegistration` payload (Table 5.2.9-1).
///
/// Spelt as the specification spells it, both where a payload member is looked
/// up and where a refusal names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, IntoStaticStr)]
#[strum(serialize_all = "camelCase")]
pub enum CsrMember {
    /// `type`.
    Type,
    /// `information`.
    Information,
    /// `mode`.
    Mode,
    /// `operations`.
    Operations,
    /// `registrationName`.
    RegistrationName,
    /// `description`.
    Description,
    /// `datasetId`.
    DatasetId,
    /// `tenant`.
    Tenant,
    /// `observationInterval`.
    ObservationInterval,
    /// `managementInterval`.
    ManagementInterval,
    /// `location`.
    Location,
    /// `observationSpace`.
    ObservationSpace,
    /// `operationSpace`.
    OperationSpace,
    /// `expiresAt`.
    ExpiresAt,
    /// `endpoint`.
    Endpoint,
    /// `accept`.
    Accept,
    /// `scope`.
    Scope,
    /// `refreshRate`.
    RefreshRate,
    /// `management`.
    Management,
    /// `contextSourceInfo`.
    ContextSourceInfo,
    /// `jsonldContext`.
    JsonldContext,
    /// `contextSourceAlias`.
    ContextSourceAlias,
}

impl CsrMember {
    /// The member name as a payload spells it.
    #[must_use]
    pub fn key(self) -> &'static str {
        self.into()
    }
}

#[cfg(test)]
mod tests {
    use crate::csr_member::CsrMember;
    use pretty_assertions::assert_eq;

    #[test]
    fn members_are_spelt_as_the_specification_spells_them() {
        assert_eq!(CsrMember::ExpiresAt.key(), "expiresAt");
        assert_eq!(CsrMember::JsonldContext.key(), "jsonldContext");
        assert_eq!(CsrMember::Type.to_string(), "type");
    }
}
