//! Typed conversions from subsystem errors into broker errors.

use crate::error::{
    AndromedaError,
    AttributeListIdParseError,
    AttributeNameLookupError,
    ConfigError,
    ContextStoreError,
    EntityMapIdParseError,
    EntityTypeListIdParseError,
    ErrorKind,
    IoError,
    JsonLdError,
    MigrateError,
    ParameterError,
    QueryError,
    RenderError,
    ReqwestError,
    SerdeJsonError,
    SetGlobalDefaultError,
    SnapshotIdParseError,
    SqlxError,
    StatementRenderError,
    TemporalStoreError,
    TenancyError,
};
use andromeda_context_commit::error::CommitError;
use andromeda_context_registry::error::ContextRegistryError;
use andromeda_entity_map::error::EntityMapError;

impl From<andromeda_postgresql_encoding::ewkb::EwkbError> for AndromedaError {
    fn from(error: andromeda_postgresql_encoding::ewkb::EwkbError) -> AndromedaError {
        AndromedaError::internal(format!("EWKB codec: {error}"))
    }
}

/// An attribute-name dictionary lookup failure is never client-caused: the
/// attribute IRI was already validated on the way in, so a failure here is a
/// broker or driver fault.
impl From<AttributeNameLookupError> for AndromedaError {
    fn from(error: AttributeNameLookupError) -> AndromedaError {
        AndromedaError::internal(format!("Attribute name lookup: {error}"))
    }
}

// -- From impls that go through ErrorKind's #[from] --------------------------

impl From<MigrateError> for AndromedaError {
    fn from(error: MigrateError) -> AndromedaError {
        ErrorKind::from(error).into()
    }
}

impl From<ConfigError> for AndromedaError {
    fn from(error: ConfigError) -> AndromedaError {
        ErrorKind::from(error).into()
    }
}

impl From<SetGlobalDefaultError> for AndromedaError {
    fn from(error: SetGlobalDefaultError) -> AndromedaError {
        ErrorKind::from(error).into()
    }
}

impl From<IoError> for AndromedaError {
    fn from(error: IoError) -> AndromedaError {
        ErrorKind::from(error).into()
    }
}

// -- Manual From impls -------------------------------------------------------

impl From<SqlxError> for AndromedaError {
    fn from(error: SqlxError) -> AndromedaError {
        let kind = if let SqlxError::Database(ref db_error) = error {
            match db_error.code().as_deref() {
                Some("23505") => ErrorKind::DatabaseUniqueViolation(error),
                Some("2201B") => ErrorKind::DatabaseRegexError(error),
                Some(_) => ErrorKind::Database(error),
                None => ErrorKind::Database(error),
            }
        } else {
            ErrorKind::Database(error)
        };
        kind.into()
    }
}

impl From<JsonLdError> for AndromedaError {
    fn from(error: JsonLdError) -> AndromedaError {
        ErrorKind::JsonLd(error).into()
    }
}

impl From<TenancyError> for AndromedaError {
    fn from(error: TenancyError) -> AndromedaError {
        ErrorKind::Tenancy(error).into()
    }
}

impl From<SerdeJsonError> for AndromedaError {
    fn from(error: SerdeJsonError) -> AndromedaError {
        ErrorKind::JsonParse(error).into()
    }
}

impl From<urn_rs::Error> for AndromedaError {
    fn from(error: urn_rs::Error) -> AndromedaError {
        ErrorKind::InvalidUrn(error).into()
    }
}

impl From<SnapshotIdParseError> for AndromedaError {
    fn from(error: SnapshotIdParseError) -> AndromedaError {
        ErrorKind::InvalidSnapshotId(error).into()
    }
}

impl From<EntityMapIdParseError> for AndromedaError {
    fn from(error: EntityMapIdParseError) -> AndromedaError {
        ErrorKind::InvalidEntityMapId(error).into()
    }
}

impl From<EntityTypeListIdParseError> for AndromedaError {
    fn from(error: EntityTypeListIdParseError) -> AndromedaError {
        ErrorKind::InvalidEntityTypeListId(error).into()
    }
}

impl From<AttributeListIdParseError> for AndromedaError {
    fn from(error: AttributeListIdParseError) -> AndromedaError {
        ErrorKind::InvalidAttributeListId(error).into()
    }
}

impl From<ParameterError> for AndromedaError {
    fn from(err: ParameterError) -> AndromedaError {
        // Rendered before the match so every arm can take `err` by value and
        // still report it; borrowing it inside the match would keep the
        // delegating arm from moving the wrapped `QueryError` out.
        let message = err.to_string();
        let kind = match err {
            // The language that owns a value already categorises its own
            // rejection, so this arm delegates rather than restating it.
            ParameterError::Query(query) => return AndromedaError::from(query),
            // Structurally malformed: the value names nothing the broker has.
            ParameterError::OptionsInvalid(_) | ParameterError::FormatInvalid(_) => ErrorKind::MalformedRequest(message),
            // Well-formed, but not a request the broker can answer.
            ParameterError::IdPatternEmpty
            | ParameterError::OrderByDirectionInvalid(_)
            | ParameterError::OrderGeometryInvalid(_)
            | ParameterError::OrderGeometryCollection
            | ParameterError::OrderFromInvalid { .. }
            | ParameterError::DropKeepExclusive
            | ParameterError::AttrsReservedMember(_)
            | ParameterError::TimerelInvalid(_)
            | ParameterError::TimePropertyInvalid(_)
            | ParameterError::TimerelTimeatRequired
            | ParameterError::AggrMethodInvalid(_)
            | ParameterError::SelectorRequired(_)
            | ParameterError::AggrMethodsRequired
            | ParameterError::EntityQueryTemporalMembers
            | ParameterError::TemporalQueryMissing
            | ParameterError::TemporalAggregationConflict
            | ParameterError::RequestTypeInvalid(_) => ErrorKind::QueryValidation(message),
        };
        kind.into()
    }
}

impl From<QueryError> for AndromedaError {
    fn from(err: QueryError) -> AndromedaError {
        let kind = match &err {
            QueryError::QEmpty
            | QueryError::QUnterminatedString
            | QueryError::QUnexpectedToken { .. }
            | QueryError::QExpected { .. }
            | QueryError::QUnexpectedChar { .. }
            | QueryError::QAttributePathTooDeep { .. }
            | QueryError::TypeExprEmpty
            | QueryError::TypeExprUnexpectedEnd
            | QueryError::TypeExprUnbalanced(_)
            | QueryError::TypeExprUnexpected { .. }
            | QueryError::TypeExprEmptyName(_)
            | QueryError::TypeExprInvalidName { .. }
            | QueryError::GeoIncomplete => ErrorKind::MalformedRequest(err.to_string()),
            QueryError::QInvalidNumber(_)
            | QueryError::QInvalidDateTime { .. }
            | QueryError::QInvalidDate { .. }
            | QueryError::QInvalidTime { .. }
            | QueryError::PatternRegex(_)
            | QueryError::GeoRel(_)
            | QueryError::IcuCollation(_)
            | QueryError::ParamIdInvalid(_)
            | QueryError::ParamIdListEmpty
            | QueryError::ParamDatasetIdInvalid(_)
            | QueryError::ScopePatternInvalid(_)
            | QueryError::ParamTimerelTimeatPair
            | QueryError::ParamAggrPeriodDurationInvalid(_)
            | QueryError::ParamEndTimeAtRequired
            | QueryError::GeoCoordinatesInvalid(_)
            | QueryError::ReferenceGeometry(_)
            | QueryError::NearWithoutDistance
            | QueryError::DistanceOrderWithoutReference => ErrorKind::QueryValidation(err.to_string()),
            // Neither the client nor the request shape caused these: a term the
            // processor accepted failed to expand, or an expanded IRI turned
            // out not to name an attribute. They are broker faults.
            QueryError::TermExpansion { .. } | QueryError::TermNotAnIri { .. } | QueryError::AttributeNameInvalid { .. } => {
                ErrorKind::Internal(err.to_string())
            }
        };
        kind.into()
    }
}

impl From<StatementRenderError> for AndromedaError {
    fn from(err: StatementRenderError) -> AndromedaError {
        match err {
            // The query itself was at fault; it is classified where it is defined.
            StatementRenderError::Query(query) => query.into(),
            // Neither the client nor the request shape caused these: a geometry
            // that already parsed failed to encode, or sea-query produced a
            // value with no typed bind. They are broker faults.
            StatementRenderError::BindConversion(_) | StatementRenderError::ReferenceGeometryEncoding(_) => ErrorKind::Internal(err.to_string()).into(),
        }
    }
}

/// A rendering failure is never client-caused: the resource was validated on
/// the way in and the representation options are typed, so a failure here is a
/// broker fault.
impl From<RenderError> for AndromedaError {
    fn from(error: RenderError) -> AndromedaError {
        ErrorKind::Rendering(error).into()
    }
}

impl From<ReqwestError> for AndromedaError {
    fn from(error: ReqwestError) -> AndromedaError {
        ErrorKind::ExternalRequest(error).into()
    }
}

impl From<CommitError> for AndromedaError {
    fn from(error: CommitError) -> AndromedaError {
        AndromedaError::internal(error.to_string())
    }
}

impl From<EntityMapError> for AndromedaError {
    fn from(error: EntityMapError) -> AndromedaError {
        match error {
            EntityMapError::NotFound { identifier } => AndromedaError::entity_map_not_found(identifier.to_string()),
            EntityMapError::EntityNotInMap { entity_identifier, .. } => AndromedaError::entity_map_not_found(entity_identifier.to_string()),
            EntityMapError::Persistence { detail } | EntityMapError::Invariant { detail } => AndromedaError::internal(detail),
        }
    }
}

impl From<ContextRegistryError> for AndromedaError {
    fn from(error: ContextRegistryError) -> AndromedaError {
        match error {
            ContextRegistryError::MissingTopLevelContext => AndromedaError::malformed_request("Payload must contain a top-level @context field"),
            ContextRegistryError::NotFound { identifier } => AndromedaError::context_not_found(identifier.to_string()),
            ContextRegistryError::CoreContextDeletion => AndromedaError::jsonld_processing("Cannot delete a core context"),
            ContextRegistryError::ReloadRequiresCached { identifier } => {
                AndromedaError::jsonld_processing(format!("Cannot reload non-cached context: {identifier}"))
            }
            ContextRegistryError::MissingSourceUrl { identifier } => {
                AndromedaError::jsonld_processing(format!("Cached context has no source URL: {identifier}"))
            }
            ContextRegistryError::ContextUnavailable { source_url, detail } => {
                AndromedaError::context_not_available(format!("Failed to re-download context from {source_url}: {detail}"))
            }
            ContextRegistryError::Persistence { detail } | ContextRegistryError::Representation { detail } | ContextRegistryError::Invariant { detail } => {
                AndromedaError::internal(detail)
            }
        }
    }
}

/// A temporal store failure is mapped to the error category that matches
/// the original call site's intent: not-found stays 404, validation stays
/// 400, and infrastructure errors stay 500.
impl From<TemporalStoreError> for AndromedaError {
    fn from(error: TemporalStoreError) -> AndromedaError {
        match error {
            TemporalStoreError::NotFound(msg) => AndromedaError::temporal_not_found(msg),
            TemporalStoreError::Validation(msg) => AndromedaError::temporal_validation(msg),
            TemporalStoreError::Database(_)
            | TemporalStoreError::Decode(_)
            | TemporalStoreError::Query(_)
            | TemporalStoreError::Encoding(_)
            | TemporalStoreError::AttributeLookup(_) => AndromedaError::temporal_store(error),
        }
    }
}

/// A context store failure preserves the PG error-code structure so the
/// HTTP layer can map unique violations to 409 and regex errors to 400
/// without re-inspecting the driver error.
impl From<ContextStoreError> for AndromedaError {
    fn from(error: ContextStoreError) -> AndromedaError {
        match error {
            ContextStoreError::Database(e) => ErrorKind::Database(e).into(),
            ContextStoreError::UniqueViolation(e) => ErrorKind::DatabaseUniqueViolation(e).into(),
            ContextStoreError::InvalidRegex(e) => ErrorKind::DatabaseRegexError(e).into(),
            ContextStoreError::Encoding(e) => AndromedaError::internal(format!("EWKB codec: {e}")),
            ContextStoreError::Render(e) => AndromedaError::internal(format!("statement render: {e}")),
            ContextStoreError::AttributeLookup(e) => AndromedaError::internal(format!("attribute name lookup: {e}")),
            ContextStoreError::ContextMapper(e) => AndromedaError::internal(format!("JSON-LD context row: {e}")),
            ContextStoreError::Migration(e) => ErrorKind::Migration(e).into(),
            ContextStoreError::Internal(msg) => AndromedaError::internal(msg),
        }
    }
}
