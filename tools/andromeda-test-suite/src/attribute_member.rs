//! The optional members of an attribute a matcher may require to be present or absent, named as the specification
//! writes them.

/// An optional member of an attribute that a matcher may require to be absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum Member {
    /// `observedAt`.
    #[strum(serialize = "observedAt")]
    ObservedAt,
    /// `expiresAt`.
    #[strum(serialize = "expiresAt")]
    ExpiresAt,
    /// `unitCode`.
    #[strum(serialize = "unitCode")]
    UnitCode,
    /// `datasetId`.
    #[strum(serialize = "datasetId")]
    DatasetId,
    /// `valueType`.
    #[strum(serialize = "valueType")]
    ValueType,
    /// `createdAt`.
    #[strum(serialize = "createdAt")]
    CreatedAt,
    /// `modifiedAt`.
    #[strum(serialize = "modifiedAt")]
    ModifiedAt,
    /// `deletedAt`, which a broker writes on an instance it reports deleted (clause 4.5.0).
    #[strum(serialize = "deletedAt")]
    DeletedAt,
    /// `instanceId` (clause 4.5.7), which a broker writes on every instance of a temporal representation.
    #[strum(serialize = "instanceId")]
    InstanceId,
    /// `languageMap`, which only a `LanguageProperty` holds (clause 4.5.18).
    #[strum(serialize = "languageMap")]
    LanguageMap,
    /// `lang`, which a language filter writes on the Property it picked one language of (clause 4.15).
    #[strum(serialize = "lang")]
    Lang,
    /// The previous member of the attribute's kind (`previousValue`, `previousObject`, `previousLanguageMap` and the
    /// others), which a notification with `showChanges` carries (clause 5.8.1).
    #[strum(serialize = "previous")]
    Previous,
}
