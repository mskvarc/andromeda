//! PostgreSQL type names as identifiers.
//!
//! Every cast names a physical type, and naming it once here keeps two casts
//! from spelling the same type differently. The schema declares no enum type:
//! closed vocabularies are `SMALLINT` registry codes, which need no cast. The array
//! variants carry their `[]` so an `UNNEST` argument and an `::integer[]` cast
//! read the element type the same way a scalar column does.

use sea_query::Iden;

/// A PostgreSQL type, as it is spelled where a statement names one.
///
/// Backs [`crate::cast`]: a value cast to
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
    /// `time`, the time-of-day type a clause 4.6.3 `Time` value casts to.
    #[iden = "time"]
    Time,
    /// `text`.
    #[iden = "text"]
    Text,
    /// `geography`, the spheroidal type `near`/distance operands cast to.
    #[iden = "geography"]
    Geography,
    /// `box2d`, a geometry's planar bounding box, which no geometry index
    /// stores.
    #[iden = "box2d"]
    Box2d,
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
    /// `jsonb`, the parsed form a column SQL navigates inside is stored as.
    #[iden = "jsonb"]
    Jsonb,
    /// `json`, the verbatim text form a document column SQL never reads inside
    /// is stored as: the server validates the text and keeps it as written.
    #[iden = "json"]
    Json,
    /// `interval`, the duration type an aggregation period binds as.
    #[iden = "interval"]
    Interval,
    /// `lquery`, the `ltree` match-pattern type (clause 4.18 scope matching).
    #[iden = "lquery"]
    Lquery,
    /// `ltree`, the label-path type a scope is stored as (clause 4.18).
    #[iden = "ltree"]
    Ltree,
    /// `uuid`, the identity type write and lease identifiers carry.
    #[iden = "uuid"]
    Uuid,
    /// `regclass`, the relation-identity type a catalogue row keys a table by.
    #[iden = "regclass"]
    Regclass,
    /// `oid`, the object identifier a `regclass` is, and which casts between it
    /// and an integer.
    #[iden = "oid"]
    Oid,
    /// `name`, the type the catalogue spells an identifier in, which a
    /// function taking a column name expects.
    #[iden = "name"]
    Name,
    /// `cstring`, the C string pseudo-type an extension function written in C
    /// may take its text arguments as.
    #[iden = "cstring"]
    Cstring,
    /// `xid8`, the 64-bit transaction identifier a write is stamped with.
    #[iden = "xid8"]
    Xid8,
    /// `float8`, double precision.
    #[iden = "float8"]
    Float8,
    /// `int4multirange`, a set of integer ranges.
    #[iden = "int4multirange"]
    Int4Multirange,
    /// `integer[]`.
    #[iden = "integer[]"]
    IntegerArray,
    /// `bigint[]`.
    #[iden = "bigint[]"]
    BigintArray,
    /// `smallint[]`.
    #[iden = "smallint[]"]
    SmallintArray,
    /// `ltree[]`, the array type a registration's scopes are stored as.
    #[iden = "ltree[]"]
    LtreeArray,
    /// `boolean[]`.
    #[iden = "boolean[]"]
    BooleanArray,
    /// `numeric[]`.
    #[iden = "numeric[]"]
    NumericArray,
    /// `text[]`.
    #[iden = "text[]"]
    TextArray,
    /// `bytea[]`.
    #[iden = "bytea[]"]
    ByteaArray,
    /// `jsonb[]`.
    #[iden = "jsonb[]"]
    JsonbArray,
    /// `json[]`.
    #[iden = "json[]"]
    JsonArray,
    /// `timestamptz[]`.
    #[iden = "timestamptz[]"]
    TimestamptzArray,
    /// `float8[]`, the array type durations measured in seconds bind as.
    #[iden = "float8[]"]
    Float8Array,
    /// `uuid[]`.
    #[iden = "uuid[]"]
    UuidArray,
    /// `sub_attribute`: one sub-attribute of an attribute instance.
    #[iden = "sub_attribute"]
    SubAttribute,
    /// `sub_attribute_tree[]`: the sub-attribute elements of many attribute
    /// instances, one composite per instance.
    #[iden = "sub_attribute_tree[]"]
    SubAttributeTreeArray,
    /// `attribute_instance_record`: one top-level attribute instance of an
    /// Entity's state.
    #[iden = "attribute_instance_record"]
    AttributeInstanceRecord,
    /// `entity_state_record`: one Entity's state beside its identifier.
    #[iden = "entity_state_record"]
    EntityStateRecord,
    /// `temporal_entry`: one entry of a temporal append intent.
    #[iden = "temporal_entry"]
    TemporalEntry,
    /// `temporal_intent[]`: the temporal append intents of one commit.
    #[iden = "temporal_intent[]"]
    TemporalIntentArray,
    /// `pending_change_event`: one change a respond-async job owes.
    #[iden = "pending_change_event"]
    PendingChangeEvent,
    /// `pending_change_event[]`: the changes one respond-async job owes.
    #[iden = "pending_change_event[]"]
    PendingChangeEventArray,
}

#[cfg(test)]
mod tests {
    use crate::{cast::cast, pg_type::PostgresType};
    use pretty_assertions::assert_eq;
    use sea_query::{Expr, Iden, PostgresQueryBuilder, Query};

    #[test]
    fn scalar_type_names_render_bare() {
        assert_eq!(PostgresType::Numeric.to_string(), "numeric");
        assert_eq!(PostgresType::Boolean.to_string(), "boolean");
        assert_eq!(PostgresType::Timestamptz.to_string(), "timestamptz");
        assert_eq!(PostgresType::Time.to_string(), "time");
        assert_eq!(PostgresType::Text.to_string(), "text");
        assert_eq!(PostgresType::Geography.to_string(), "geography");
        assert_eq!(PostgresType::Box2d.to_string(), "box2d");
        assert_eq!(PostgresType::Interval.to_string(), "interval");
        assert_eq!(PostgresType::Lquery.to_string(), "lquery");
        assert_eq!(PostgresType::Jsonb.to_string(), "jsonb");
        assert_eq!(PostgresType::Name.to_string(), "name");
    }

    /// An array type carries its `[]`, so an `UNNEST` argument reads the element
    /// type exactly as the matching scalar column does.
    #[test]
    fn array_type_names_carry_their_brackets() {
        assert_eq!(PostgresType::IntegerArray.to_string(), "integer[]");
        assert_eq!(PostgresType::BigintArray.to_string(), "bigint[]");
        assert_eq!(PostgresType::NumericArray.to_string(), "numeric[]");
        assert_eq!(PostgresType::TextArray.to_string(), "text[]");
        assert_eq!(PostgresType::ByteaArray.to_string(), "bytea[]");
        assert_eq!(PostgresType::JsonbArray.to_string(), "jsonb[]");
        assert_eq!(PostgresType::TimestamptzArray.to_string(), "timestamptz[]");
        assert_eq!(PostgresType::Float8Array.to_string(), "float8[]");
    }

    #[test]
    fn json_and_json_arrays_render_their_postgresql_names() {
        let scalar = Query::select().expr(cast(Expr::val("{}"), PostgresType::Json)).to_string(PostgresQueryBuilder);
        let array = Query::select()
            .expr(cast(Expr::val("{}"), PostgresType::JsonArray))
            .to_string(PostgresQueryBuilder);
        assert_eq!(scalar, "SELECT '{}'::json");
        assert_eq!(array, "SELECT '{}'::json[]");
    }

    #[test]
    fn composite_type_names_render_as_the_migrations_declare_them() {
        assert_eq!(PostgresType::SubAttribute.to_string(), "sub_attribute");
        assert_eq!(PostgresType::SubAttributeTreeArray.to_string(), "sub_attribute_tree[]");
        assert_eq!(PostgresType::AttributeInstanceRecord.to_string(), "attribute_instance_record");
        assert_eq!(PostgresType::EntityStateRecord.to_string(), "entity_state_record");
        assert_eq!(PostgresType::TemporalEntry.to_string(), "temporal_entry");
        assert_eq!(PostgresType::TemporalIntentArray.to_string(), "temporal_intent[]");
        assert_eq!(PostgresType::PendingChangeEventArray.to_string(), "pending_change_event[]");
    }
}
