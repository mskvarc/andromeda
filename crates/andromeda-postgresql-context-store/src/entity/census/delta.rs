//! The census changes a set-write statement adds beside its `entity_search` write.
//!
//! A statement that inserts, rewrites or deletes `entity_search` rows returns
//! their images ([`SearchImage`]) from a CTE, and the statements built here read
//! that CTE and insert into the three census ledgers what the write changed:
//! every Entity of a `new` image counted in under each of its types, every one
//! of an `old` image counted out, and every attribute instance likewise, per
//! type, name and family and per name alone.
//!
//! The images are first grouped by their whole shape (the type array and the
//! two instance arrays), with the signed number of rows of each shape. A batch
//! of entities from one producer holds a handful of shapes however many
//! entities it holds (the instance arrays are stored in one canonical order, so
//! entities that carry the same instances carry the same arrays), which makes
//! the expansion into types and instances a few rows long rather than one row
//! per entity. The grouping also nets a rewrite out: a row whose types and
//! instances did not change has a pre-image and a post-image of the same shape,
//! and they cancel inside the group. Each ledger's changes are summed per key
//! again after the expansion, and only a change that is not zero is inserted.
//!
//! An instance's name and family are read from the two aligned image arrays in
//! lockstep: two set-returning functions in one select list advance together,
//! which pairs the elements position by position because the stored arrays
//! always have the same length (`chk_entity_search_attribute_instances_aligned`).

use crate::{
    entity::write::search_image::{ImageSide, SearchImage},
    error::ContextStoreError,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::census::{AttributeCensus, EntityTypeAttributeCensus, EntityTypeCensus};
use andromeda_postgresql_seaquery::{
    literal::{int_literal, true_literal},
    unnest::unnest_of,
};
use sea_query::{
    CommonTableExpression,
    DynIden,
    Expr,
    ExprTrait,
    Func,
    Iden,
    InsertStatement,
    IntoIden,
    JoinType,
    Query,
    SelectStatement,
    SimpleExpr,
    UnionType,
};

/// Which images the source CTE of a census change returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CensusChange {
    /// The rows an insert created: their `new` image is counted in.
    Added,
    /// The rows a delete removed: their `old` image is counted out.
    Removed,
    /// The rows an update rewrote: the `old` image is counted out and the `new`
    /// one in.
    Replaced,
}

impl CensusChange {
    /// The images of the source rows as signed shape rows: `arm` built for
    /// every side the change counts, with the sign it counts it by, the
    /// post-image in and the pre-image out, joined with `UNION ALL`.
    fn sides(self, arm: impl Fn(ImageSide, i32) -> SelectStatement) -> SelectStatement {
        match self {
            CensusChange::Added => arm(ImageSide::New, 1),
            CensusChange::Removed => arm(ImageSide::Old, -1),
            CensusChange::Replaced => {
                let mut sides = arm(ImageSide::New, 1);
                sides.union(UnionType::All, arm(ImageSide::Old, -1));
                sides
            }
        }
    }
}

/// The CTEs a census change adds to a statement.
#[derive(Iden, Clone, Copy)]
pub(crate) enum CensusCte {
    /// The signed number of written rows of every shape.
    #[iden = "census_shapes"]
    Shapes,
    /// The `entity_type_census` insert.
    #[iden = "census_types"]
    Types,
    /// The `entity_type_attribute_census` insert.
    #[iden = "census_type_attributes"]
    TypeAttributes,
    /// The `attribute_census` insert.
    #[iden = "census_attributes"]
    Attributes,
}

/// The columns of [`CensusCte::Shapes`] and of the signed rows it groups.
#[derive(Iden, Clone, Copy)]
enum Shape {
    /// The derived table of signed rows, `signed`.
    #[iden = "signed"]
    Signed,
    /// The type keys of a shape.
    #[iden = "types"]
    Types,
    /// The attribute instance name keys of a shape.
    #[iden = "names"]
    Names,
    /// The attribute instance family codes of a shape.
    #[iden = "kinds"]
    Kinds,
    /// `+1` for a row counted in, `-1` for one counted out.
    #[iden = "sign"]
    Sign,
    /// The signed number of rows of a shape.
    #[iden = "rows"]
    Rows,
}

/// The expanded keys a ledger's changes are summed under.
#[derive(Iden, Clone, Copy)]
enum Expansion {
    /// The type key.
    TypeKey,
    /// The attribute name key.
    NameKey,
    /// The attribute family code.
    Kind,
}

/// The lateral expansion of a shape's type keys, `t`.
#[derive(Iden, Clone, Copy)]
#[iden = "t"]
struct Types;

/// The lateral expansion of a shape's attribute instances, `i`.
#[derive(Iden, Clone, Copy)]
#[iden = "i"]
struct Instances;

/// The statements one census change is made of, in the order a `WITH` runs
/// them: the shapes first, then one insert per ledger.
pub(crate) struct CensusStatements {
    /// The signed shapes of the written rows.
    shapes: SelectStatement,
    /// The `entity_type_census` insert.
    types: InsertStatement,
    /// The `entity_type_attribute_census` insert.
    type_attributes: InsertStatement,
    /// The `attribute_census` insert.
    attributes: InsertStatement,
}

impl CensusStatements {
    /// The census change of the rows `source` returns, written into the
    /// ledgers of `target`'s branch.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::StatementBuild`] if a ledger insert ever stops
    /// matching its column list, which the fixed shapes here make unreachable.
    pub(crate) fn of(target: &ContextTarget, source: impl Iden + Copy + 'static, change: CensusChange) -> Result<CensusStatements, ContextStoreError> {
        let source = source.into_iden();
        Ok(CensusStatements {
            shapes: shapes(change.sides(|side, sign| signed_rows(&source, side, sign))),
            types: ledger_insert(
                target,
                EntityTypeCensus::Table,
                &[EntityTypeCensus::EntityTypeNameKey.into_iden(), EntityTypeCensus::EntityDelta.into_iden()],
                &[(Types.into_iden(), Expansion::TypeKey)],
                expanded(ExpandTypes::Yes, ExpandInstances::No),
            )?,
            type_attributes: ledger_insert(
                target,
                EntityTypeAttributeCensus::Table,
                &[
                    EntityTypeAttributeCensus::EntityTypeNameKey.into_iden(),
                    EntityTypeAttributeCensus::AttributeNameKey.into_iden(),
                    EntityTypeAttributeCensus::AttributeKind.into_iden(),
                    EntityTypeAttributeCensus::InstanceDelta.into_iden(),
                ],
                &[
                    (Types.into_iden(), Expansion::TypeKey),
                    (Instances.into_iden(), Expansion::NameKey),
                    (Instances.into_iden(), Expansion::Kind),
                ],
                expanded(ExpandTypes::Yes, ExpandInstances::Yes),
            )?,
            attributes: ledger_insert(
                target,
                AttributeCensus::Table,
                &[AttributeCensus::AttributeNameKey.into_iden(), AttributeCensus::InstanceDelta.into_iden()],
                &[(Instances.into_iden(), Expansion::NameKey)],
                expanded(ExpandTypes::No, ExpandInstances::Yes),
            )?,
        })
    }

    /// The census change as the `WITH` entries a sea-query statement appends.
    pub(crate) fn ctes(self) -> [CommonTableExpression; 4] {
        [
            CommonTableExpression::new().table_name(CensusCte::Shapes).query(self.shapes).to_owned(),
            CommonTableExpression::new().table_name(CensusCte::Types).query(self.types).to_owned(),
            CommonTableExpression::new()
                .table_name(CensusCte::TypeAttributes)
                .query(self.type_attributes)
                .to_owned(),
            CommonTableExpression::new().table_name(CensusCte::Attributes).query(self.attributes).to_owned(),
        ]
    }
}

/// One side of the source rows as signed shape rows:
/// `SELECT src.<types> AS types, src.<names> AS names, src.<kinds> AS kinds, <sign> AS sign FROM src`.
fn signed_rows(source: &DynIden, side: ImageSide, sign: i32) -> SelectStatement {
    let column = |image: SearchImage| Expr::col((source.clone(), image));
    Query::select()
        .expr_as(column(side.types()), Shape::Types)
        .expr_as(column(side.instance_names()), Shape::Names)
        .expr_as(column(side.instance_kinds()), Shape::Kinds)
        .expr_as(int_literal(i64::from(sign)), Shape::Sign)
        .from(source.clone())
        .take()
}

/// `SELECT types, names, kinds, SUM(sign) AS rows FROM (<signed rows>) AS signed
/// GROUP BY types, names, kinds HAVING SUM(sign) <> 0`.
fn shapes(signed: SelectStatement) -> SelectStatement {
    let total = || SimpleExpr::from(Func::sum(Expr::col((Shape::Signed, Shape::Sign))));
    let mut shapes = Query::select();
    for column in [Shape::Types, Shape::Names, Shape::Kinds] {
        shapes.column((Shape::Signed, column)).group_by_col((Shape::Signed, column));
    }
    shapes
        .expr_as(total(), Shape::Rows)
        .from_subquery(signed, Shape::Signed)
        .and_having(total().ne(int_literal(0)));
    shapes.take()
}

/// Whether a ledger's expansion crosses the shapes with their type keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpandTypes {
    /// The ledger counts per type.
    Yes,
    /// The ledger counts whatever the types.
    No,
}

/// Whether a ledger's expansion crosses the shapes with their instances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpandInstances {
    /// The ledger counts instances.
    Yes,
    /// The ledger counts entities.
    No,
}

/// `FROM census_shapes` with the laterals a ledger expands.
fn expanded(types: ExpandTypes, instances: ExpandInstances) -> SelectStatement {
    let mut rows = Query::select();
    rows.from(CensusCte::Shapes);
    match types {
        ExpandTypes::Yes => {
            let type_keys = Query::select()
                .expr_as(SimpleExpr::from(unnest_of(Expr::col((CensusCte::Shapes, Shape::Types)))), Expansion::TypeKey)
                .take();
            rows.join_lateral(JoinType::InnerJoin, type_keys, Types, true_literal());
        }
        ExpandTypes::No => {}
    }
    match instances {
        ExpandInstances::Yes => {
            let instance_keys = Query::select()
                .expr_as(SimpleExpr::from(unnest_of(Expr::col((CensusCte::Shapes, Shape::Names)))), Expansion::NameKey)
                .expr_as(SimpleExpr::from(unnest_of(Expr::col((CensusCte::Shapes, Shape::Kinds)))), Expansion::Kind)
                .take();
            rows.join_lateral(JoinType::InnerJoin, instance_keys, Instances, true_literal());
        }
        ExpandInstances::No => {}
    }
    rows.take()
}

/// `INSERT INTO <ledger> (<columns>) SELECT <keys>, SUM(census_shapes.rows)
/// FROM <expanded> GROUP BY <keys> HAVING SUM(census_shapes.rows) <> 0`.
fn ledger_insert(
    target: &ContextTarget,
    ledger: impl Iden + 'static,
    columns: &[DynIden],
    keys: &[(DynIden, Expansion)],
    mut rows: SelectStatement,
) -> Result<InsertStatement, ContextStoreError> {
    let total = || SimpleExpr::from(Func::sum(Expr::col((CensusCte::Shapes, Shape::Rows))));
    for (relation, key) in keys {
        rows.column((relation.clone(), *key)).group_by_col((relation.clone(), *key));
    }
    rows.expr(total()).and_having(total().ne(int_literal(0)));
    let mut insert = Query::insert();
    insert
        .into_table(table_of(target, ledger))
        .columns(columns.iter().cloned())
        .select_from(rows.take())?;
    Ok(insert)
}

#[cfg(test)]
mod tests {
    use crate::entity::census::delta::{CensusChange, CensusCte, CensusStatements};
    use andromeda_context_space::context_target::ContextTarget;
    use pretty_assertions::assert_eq;
    use sea_query::{Iden, PostgresQueryBuilder};

    /// The source CTE the statements under test read, named as the entity
    /// statements name theirs.
    #[derive(Iden, Clone, Copy)]
    #[iden = "srch"]
    struct Srch;

    /// Each `WITH` entry's name beside its rendered statement.
    fn statements_of(statements: &CensusStatements) -> [(CensusCte, String); 4] {
        [
            (CensusCte::Shapes, statements.shapes.to_string(PostgresQueryBuilder)),
            (CensusCte::Types, statements.types.to_string(PostgresQueryBuilder)),
            (CensusCte::TypeAttributes, statements.type_attributes.to_string(PostgresQueryBuilder)),
            (CensusCte::Attributes, statements.attributes.to_string(PostgresQueryBuilder)),
        ]
    }

    fn rendered(change: CensusChange) -> Vec<(String, String)> {
        let statements = CensusStatements::of(&ContextTarget::Live, Srch, change).expect("the census statements build");
        statements_of(&statements).into_iter().map(|(name, sql)| (name.to_string(), sql)).collect()
    }

    /// The written rows are grouped by shape once, and every ledger expands
    /// the few shapes rather than the rows.
    #[test]
    fn an_insert_groups_the_new_images_by_shape_before_any_ledger_expands_them() {
        let statements = rendered(CensusChange::Added);
        let names: Vec<&str> = statements.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["census_shapes", "census_types", "census_type_attributes", "census_attributes"]);
        let (_, shapes) = &statements[0];
        assert_eq!(
            shapes,
            concat!(
                r#"SELECT "signed"."types", "signed"."names", "signed"."kinds", SUM("signed"."sign") AS "rows" "#,
                r#"FROM (SELECT "srch"."new_types" AS "types", "srch"."new_instance_names" AS "names", "srch"."new_instance_kinds" AS "kinds", 1 AS "sign" FROM "srch") AS "signed" "#,
                r#"GROUP BY "signed"."types", "signed"."names", "signed"."kinds" HAVING SUM("signed"."sign") <> (0)"#
            )
        );
        for (_, ledger) in &statements[1..] {
            assert!(ledger.contains(r#"FROM "census_shapes""#), "{ledger}");
            assert!(!ledger.contains(r#""srch""#), "a ledger reads the shapes only: {ledger}");
            assert!(ledger.ends_with(r#"HAVING SUM("census_shapes"."rows") <> (0)"#), "{ledger}");
        }
    }

    #[test]
    fn a_delete_counts_the_old_images_out() {
        let statements = rendered(CensusChange::Removed);
        assert!(statements[0].1.contains(r#""srch"."old_types" AS "types""#), "{}", statements[0].1);
        assert!(statements[0].1.contains("-1 AS \"sign\""), "{}", statements[0].1);
        assert!(!statements[0].1.contains("new_"), "a delete has no post-image: {}", statements[0].1);
    }

    /// A rewrite counts both images; an unchanged row nets out inside its
    /// shape group, so no separate filter is needed.
    #[test]
    fn a_rewrite_signs_both_images_into_one_grouping() {
        let statements = rendered(CensusChange::Replaced);
        let (_, shapes) = &statements[0];
        assert!(shapes.contains("UNION ALL"), "{shapes}");
        assert!(
            shapes.contains(r#""srch"."new_types" AS "types""#) && shapes.contains(r#""srch"."old_types" AS "types""#),
            "{shapes}"
        );
    }

    /// An instance's name and family advance together in one select list, and
    /// the types are crossed with them in a lateral of their own.
    #[test]
    fn the_per_type_ledger_crosses_each_shapes_types_with_its_aligned_instances() {
        let statements = rendered(CensusChange::Added);
        let (_, sql) = &statements[2];
        assert!(
            sql.starts_with(r#"INSERT INTO "entity_type_attribute_census" ("entity_type_name_key", "attribute_name_key", "attribute_kind", "instance_delta") SELECT "t"."type_key", "i"."name_key", "i"."kind", SUM("census_shapes"."rows")"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"INNER JOIN LATERAL (SELECT UNNEST("census_shapes"."types") AS "type_key") AS "t" ON TRUE"#),
            "{sql}"
        );
        assert!(
            sql.contains(
                r#"INNER JOIN LATERAL (SELECT UNNEST("census_shapes"."names") AS "name_key", UNNEST("census_shapes"."kinds") AS "kind") AS "i" ON TRUE"#
            ),
            "{sql}"
        );
        assert!(sql.contains(r#"GROUP BY "t"."type_key", "i"."name_key", "i"."kind""#), "{sql}");
    }

    /// The entity ledger never expands instances, the attribute ledger never
    /// expands types.
    #[test]
    fn each_ledger_expands_only_what_it_counts() {
        let statements = rendered(CensusChange::Added);
        assert!(!statements[1].1.contains(r#"AS "i""#), "{}", statements[1].1);
        assert!(!statements[3].1.contains(r#"AS "t""#), "{}", statements[3].1);
    }

    #[test]
    fn a_snapshot_branch_keeps_its_census_in_the_branch_schema() {
        use andromeda_context_space::{snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
        let key = SnapshotKey::try_from(5).expect("positive key");
        let target = ContextTarget::Snapshot(SnapshotTarget::staging(key, "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri")));
        let statements = statements_of(&CensusStatements::of(&target, Srch, CensusChange::Added).expect("builds"));
        for (ledger, (_, sql)) in ["entity_type_census", "entity_type_attribute_census", "attribute_census"]
            .iter()
            .zip(&statements[1..])
        {
            assert!(sql.starts_with(&format!(r#"INSERT INTO "snap_5_stg"."{ledger}""#)), "{sql}");
        }
    }
}
