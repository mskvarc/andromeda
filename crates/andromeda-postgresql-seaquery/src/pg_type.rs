//! PostgreSQL type names as identifiers.
//!
//! A cast and an enum-label write both name a physical type, and naming it once
//! here keeps the two from spelling the same type differently. The array
//! variants carry their `[]` so an `UNNEST` argument and an `::integer[]` cast
//! read the element type the same way a scalar column does.

use sea_query::Iden;

/// A PostgreSQL type, as it is spelled where a statement names one.
///
/// Backs [`crate::cast`] and the enum-label writes: a value cast to
/// [`PostgresType::Numeric`] and a column typed `numeric` refer to one type, so
/// the token is written in exactly one place.
#[derive(Iden, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostgresType {
    /// Arbitrary-precision `numeric`; the value column NGSI-LD numbers land in.
    #[iden = "numeric"]
    Numeric,
    /// `boolean`.
    #[iden = "boolean"]
    Boolean,
    /// `timestamptz`, the instant type every observed/modified time carries.
    #[iden = "timestamptz"]
    Timestamptz,
    /// `text`.
    #[iden = "text"]
    Text,
    /// `geography`, the spheroidal type `near`/distance operands cast to.
    #[iden = "geography"]
    Geography,
    /// `integer`.
    #[iden = "integer"]
    Integer,
    /// `bigint`.
    #[iden = "bigint"]
    Bigint,
    /// `smallint`.
    #[iden = "smallint"]
    Smallint,
    /// `bytea`, the wire type EWKB geometry binds as.
    #[iden = "bytea"]
    Bytea,
    /// `jsonb`.
    #[iden = "jsonb"]
    Jsonb,
    /// `interval`, the duration type an aggregation period binds as.
    #[iden = "interval"]
    Interval,
    /// `lquery`, the `ltree` match-pattern type (clause 4.18 scope matching).
    #[iden = "lquery"]
    Lquery,
    /// `ltree`, the label-path type a scope is stored as (clause 4.18).
    #[iden = "ltree"]
    Ltree,
    /// `jsonld_context_kind`, the enum a stored Context's kind carries.
    #[iden = "jsonld_context_kind"]
    JsonldContextKind,
    /// `snapshot_status`, the enum a snapshot's lifecycle state carries.
    #[iden = "snapshot_status"]
    SnapshotStatus,
    /// `subscription_status`, the enum a subscription's state carries.
    #[iden = "subscription_status"]
    SubscriptionStatus,
    /// `csr_mode`, the enum a registration's forwarding mode carries.
    #[iden = "csr_mode"]
    CsrMode,
    /// `uuid`, the identity type write and lease identifiers carry.
    #[iden = "uuid"]
    Uuid,
    /// `integer[]`.
    #[iden = "integer[]"]
    IntegerArray,
    /// `bigint[]`.
    #[iden = "bigint[]"]
    BigintArray,
    /// `smallint[]`.
    #[iden = "smallint[]"]
    SmallintArray,
    /// `boolean[]`.
    #[iden = "boolean[]"]
    BooleanArray,
    /// `text[]`.
    #[iden = "text[]"]
    TextArray,
    /// `bytea[]`.
    #[iden = "bytea[]"]
    ByteaArray,
    /// `jsonb[]`.
    #[iden = "jsonb[]"]
    JsonbArray,
    /// `timestamptz[]`.
    #[iden = "timestamptz[]"]
    TimestamptzArray,
    /// `uuid[]`.
    #[iden = "uuid[]"]
    UuidArray,
}

#[cfg(test)]
mod tests {
    use crate::pg_type::PostgresType;
    use sea_query::Iden;

    #[test]
    fn scalar_type_names_render_bare() {
        assert_eq!(PostgresType::Numeric.to_string(), "numeric");
        assert_eq!(PostgresType::Boolean.to_string(), "boolean");
        assert_eq!(PostgresType::Timestamptz.to_string(), "timestamptz");
        assert_eq!(PostgresType::Text.to_string(), "text");
        assert_eq!(PostgresType::Geography.to_string(), "geography");
        assert_eq!(PostgresType::Interval.to_string(), "interval");
        assert_eq!(PostgresType::Lquery.to_string(), "lquery");
        assert_eq!(PostgresType::Jsonb.to_string(), "jsonb");
    }

    /// An array type carries its `[]`, so an `UNNEST` argument reads the element
    /// type exactly as the matching scalar column does.
    #[test]
    fn array_type_names_carry_their_brackets() {
        assert_eq!(PostgresType::IntegerArray.to_string(), "integer[]");
        assert_eq!(PostgresType::BigintArray.to_string(), "bigint[]");
        assert_eq!(PostgresType::TextArray.to_string(), "text[]");
        assert_eq!(PostgresType::ByteaArray.to_string(), "bytea[]");
        assert_eq!(PostgresType::JsonbArray.to_string(), "jsonb[]");
        assert_eq!(PostgresType::TimestamptzArray.to_string(), "timestamptz[]");
    }
}
