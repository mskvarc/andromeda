//! The version a guarded write compares and the count every write of the
//! current state moves.
//!
//! An update or a delete that states the version its caller read stages that
//! version beside the row, and lands only where the identity row still stands
//! as read; every write of the current state moves the write count, so a later
//! write never satisfies an expectation an earlier one did.

use crate::entity::write::staging::StagedColumn;
use andromeda_context_commit::entity_version::EntityVersion;
use andromeda_postgresql_schema::current::Entities;
use andromeda_postgresql_seaquery::{pg_array::nullable_array, pg_type::PostgresType, unnest::UnnestColumn};
use sea_query::{Cond, Expr, ExprTrait, IntoIden, SimpleExpr};

/// The two staging columns of a version expectation, the key and the count.
pub(super) fn expectation_columns() -> [UnnestColumn; 2] {
    [
        UnnestColumn::new(StagedColumn::ExpectedEntityKey, PostgresType::BigintArray),
        UnnestColumn::new(StagedColumn::ExpectedWriteCount, PostgresType::BigintArray),
    ]
}

/// The two arrays staging `expected` in [`expectation_columns`] order: a
/// `None` entry is NULL in both.
pub(super) fn expectation_arrays(expected: &[Option<EntityVersion>]) -> [SimpleExpr; 2] {
    [
        nullable_array(expected.iter().map(|version| version.map(EntityVersion::entity_key))),
        nullable_array(expected.iter().map(|version| version.map(EntityVersion::write_count))),
    ]
}

/// The write count a write of the identity row named `entities` leaves:
/// `<entities>.write_count + 1`. Every write of the current state moves it, so
/// a later write never matches an expectation an earlier one satisfied.
pub(super) fn next_write_count(entities: impl IntoIden) -> SimpleExpr {
    Expr::col((entities.into_iden(), Entities::WriteCount)).add(SimpleExpr::Constant(1.into()))
}

/// The precondition the update and the delete carry:
/// `<s>.expected_entity_key IS NULL OR (<entities>.entity_key = <s>.expected_entity_key
/// AND <entities>.write_count = <s>.expected_write_count)`, with `s` the
/// relation the statement carries the expectation in (`d` for the delete,
/// `proj` for the update) and `entities` the name the statement reads the
/// identity row under.
///
/// A null expectation matches whatever is there; a non-null one matches only a
/// row still standing as the caller read it, which is what turns a
/// read-modify-write into a compare-and-set rather than a lost update. The
/// version compared is the row's key and write count, not its clause 4.8
/// `modifiedAt`: two writes accepted in the same microsecond stamp the same
/// instant but never leave the same count.
pub(super) fn expectation(staging: impl IntoIden, entities_alias: impl IntoIden) -> Cond {
    // clone: each alias qualifies more than one column
    let (staging, entities_alias) = (staging.into_iden(), entities_alias.into_iden());
    Cond::any().add(Expr::col((staging.clone(), StagedColumn::ExpectedEntityKey)).is_null()).add(
        Cond::all()
            .add(Expr::col((entities_alias.clone(), Entities::EntityKey)).equals((staging.clone(), StagedColumn::ExpectedEntityKey)))
            .add(Expr::col((entities_alias, Entities::WriteCount)).equals((staging, StagedColumn::ExpectedWriteCount))),
    )
}
