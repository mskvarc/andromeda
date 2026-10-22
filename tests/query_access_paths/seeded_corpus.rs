//! A production-shaped corpus written straight into the schema, and the
//! `EXPLAIN` reader the plan assertions share.
//!
//! Access-path assertions need enough rows for the planner to prefer an index
//! over a sequential scan, and they need the statistics that decision reads.
//! Creating that corpus through the API would take minutes per test, so the
//! rows are written directly and cleared again by the exclusive serialized
//! reset that runs before each test in this suite.

use andromeda_postgresql_schema::current::{CurrentAttributes, Entities, EntitySearch, EntityTypeMembers};
use andromeda_postgresql_seaquery::{
    cast::cast,
    catalog_function::{generate_series, lpad},
    clock::now,
    command::Command,
    explain::{Explain, ExplainOption},
    literal::text_literal,
    maintenance::{MaintenanceSchema, analyze as analyze_relation},
    pg_array::array_literal,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    postgis::st_point_wgs84,
    setting::{Setting, SettingAssignment, SettingScope},
    statement::Statement,
    unnest::{UnnestAliased, UnnestColumn, unnest_call},
};
use andromeda_test_fixtures::high_entropy_text::high_entropy_text;
use andromeda_test_harness::dictionary_seed::{attribute_name_key, entity_type_name_key};
use sea_query::{Alias, CaseStatement, CommonTableExpression, Expr, ExprTrait, InsertStatement, IntoIden, Query, SimpleExpr, WithClause, WithQuery};
use sqlx::{Executor, PgPool, Postgres};

/// Identifier prefix every resource this suite seeds carries.
///
/// The suite holds the exclusive serialized lease, which resets the tenant to a
/// clean slate before each test and serializes every holder, so one fixed
/// prefix suffices: no per-test uniqueness is needed, the seeded corpus is the
/// only data in the analyzed tables, and every unscoped `EXPLAIN` plan is
/// therefore deterministic.
pub const SEED_PREFIX: &str = "urn:ngsi-ld:test:query-access-paths";

/// Entities written per seeded corpus.
pub const SEEDED_ENTITIES: i64 = 20_000;

/// Type carried by every 997th seeded Entity, selective enough that an index
/// path is the cheaper plan.
pub const RARE_TYPE: &str = "https://example.test/query-access-paths/Rare";

/// Type carried by every other seeded Entity.
pub const COMMON_TYPE: &str = "https://example.test/query-access-paths/Building";

/// Expanded IRI of the seeded numeric attribute.
pub const READING_ATTRIBUTE: &str = "https://example.test/query-access-paths/reading";

/// Expanded IRI of the seeded `GeoProperty` attribute.
pub const ENTRANCE_ATTRIBUTE: &str = "https://example.test/query-access-paths/entrance";

/// Expanded IRI of the attribute every seeded Entity carries, so an `attrs`
/// term on it selects the whole corpus.
pub const CITY_ATTRIBUTE: &str = "https://example.test/query-access-paths/city";

/// Expanded IRI of the attribute every 1000th seeded Entity carries, selective
/// enough that an `attrs` term on it prefers the index.
pub const ALARM_ATTRIBUTE: &str = "https://example.test/query-access-paths/alarm";

/// Expanded IRI of the seeded Relationship, whose object every ten Entities
/// share, so an equality on one object is selective.
pub const OWNER_ATTRIBUTE: &str = "https://example.test/query-access-paths/owner";

/// Identifier prefix of the seeded Relationship objects.
pub const OWNER_PREFIX: &str = "urn:ngsi-ld:test:query-access-paths:Owner:";

/// Expanded IRI of the attribute whose seeded values are all longer than the
/// 2048-octet indexed bound, so every one of them is projected into
/// `value_long_text`.
pub const ESSAY_ATTRIBUTE: &str = "https://example.test/query-access-paths/essay";

/// Seeded Entities carrying an essay, the first of the corpus by ordinal.
///
/// A handful of overflow rows is cheaper to filter on the heap from the
/// partial B-tree than to probe through the trigram GIN, so the planner only
/// reaches for the trigram indexes once the overflow population is in the
/// thousands; this is comfortably past that crossover, which lies between one
/// and two thousand rows for this corpus.
const SEEDED_ESSAYS: u64 = 3_000;

/// Octets of every seeded essay: past the 2048-octet bound, short of anything
/// that would make the corpus slow to write.
const ESSAY_LENGTH: usize = 3000;

/// Ordinal of the essay the probe substring is cut from.
const PROBED_ESSAY: usize = 7;

/// Characters of the probe substring, enough trigrams that the pattern is
/// selective.
const PROBE_LENGTH: usize = 16;

/// The dictionary identifiers of a seeded corpus, resolved once so a plan
/// assertion binds an integer instead of joining the dictionary.
pub struct SeededCorpus {
    /// Identifier prefix every seeded Entity carries.
    prefix: String,
    /// Dictionary identifier of [`READING_ATTRIBUTE`].
    reading: i32,
    /// Dictionary identifier of [`ENTRANCE_ATTRIBUTE`].
    entrance: i32,
    /// Type dictionary identifier of [`RARE_TYPE`].
    rare_type: i32,
    /// Type dictionary identifier of [`COMMON_TYPE`].
    common_type: i32,
    /// Dictionary identifier of [`CITY_ATTRIBUTE`].
    city: i32,
    /// Dictionary identifier of [`ALARM_ATTRIBUTE`].
    alarm: i32,
    /// Dictionary identifier of [`OWNER_ATTRIBUTE`].
    owner: i32,
}

impl SeededCorpus {
    /// Borrow the shared identifier prefix.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// Dictionary identifier of the seeded numeric attribute.
    pub const fn reading(&self) -> i32 {
        self.reading
    }

    /// Dictionary identifier of the seeded `GeoProperty` attribute.
    pub const fn entrance(&self) -> i32 {
        self.entrance
    }

    /// Type dictionary identifier of the rare seeded type.
    pub const fn rare_type(&self) -> i32 {
        self.rare_type
    }

    /// Type dictionary identifier of the common seeded type.
    pub const fn common_type(&self) -> i32 {
        self.common_type
    }

    /// Dictionary identifier of the attribute every seeded Entity carries.
    pub const fn city(&self) -> i32 {
        self.city
    }

    /// Dictionary identifier of the rare seeded attribute.
    pub const fn alarm(&self) -> i32 {
        self.alarm
    }

    /// Dictionary identifier of the seeded Relationship.
    pub const fn owner(&self) -> i32 {
        self.owner
    }

    /// Identifier of one seeded Entity by ordinal.
    pub fn entity_id(&self, ordinal: usize) -> String {
        format!("{}:seed:{ordinal:06}", self.prefix)
    }
}

/// The overflow strings seeded beside a corpus, and the identities a plan
/// assertion binds.
pub struct SeededEssays {
    /// Dictionary identifier of [`ESSAY_ATTRIBUTE`].
    attribute: i32,
    /// A substring from the middle of one seeded essay, present in no other.
    probe: String,
}

impl SeededEssays {
    /// Dictionary identifier of the overflowing attribute.
    pub const fn attribute(&self) -> i32 {
        self.attribute
    }

    /// Borrow the substring a pattern over the essays probes for.
    pub fn probe(&self) -> &str {
        &self.probe
    }
}

/// The object of the seeded Relationship ten Entities share, by ordinal.
pub fn owner_object(ordinal: usize) -> String {
    format!("{OWNER_PREFIX}{ordinal}")
}

/// The common table expression numbering the corpus identifiers.
const SEEDED: &str = "seeded";

/// The common table expression holding the inserted Entity rows.
const INSERTED: &str = "ins";

/// The common table expression pairing each inserted Entity with its type.
const TYPED: &str = "typed";

/// `CASE WHEN column % modulus = 0 THEN rare ELSE common END`.
fn every_nth(column: (Alias, Alias), modulus: i64, rare: SimpleExpr, common: SimpleExpr) -> SimpleExpr {
    CaseStatement::new().case(Expr::col(column).modulo(modulus).eq(0), rare).finally(common).into()
}

/// The Entity rows of the corpus. Every 997th Entity is of the rare type, and
/// each type is stated on the search row and as a membership.
fn entity_corpus(prefix: &str, rare_type: i32, common_type: i32, count: i64, city: i32, alarm: i32) -> WithQuery {
    let identifier = cast(Expr::val(prefix), PostgresType::Text)
        .concat_text(text_literal(":seed:"))
        .concat_text(lpad(cast(Expr::col(Alias::new("g")), PostgresType::Text), Expr::val(6), text_literal("0")));
    let seeded = Query::select()
        .column(Alias::new("g"))
        .expr_as(identifier, Alias::new("id"))
        .from_function(generate_series(Expr::val(1), cast(Expr::val(count), PostgresType::Bigint)), Alias::new("g"))
        .to_owned();
    let inserted = Query::insert()
        .into_table(Entities::Table)
        .columns([Entities::Id])
        .select_from(Query::select().column(Alias::new("id")).from(Alias::new(SEEDED)).to_owned())
        .expect("the projection matches the column list")
        .returning(Query::returning().columns([Entities::EntityKey, Entities::Id]))
        .to_owned();
    let typed = Query::select()
        .column((Alias::new(INSERTED), Entities::EntityKey))
        .column((Alias::new(INSERTED), Entities::Id))
        .column((Alias::new(SEEDED), Alias::new("g")))
        .expr_as(
            every_nth(
                (Alias::new(SEEDED), Alias::new("g")),
                997,
                cast(Expr::val(rare_type), PostgresType::Integer),
                cast(Expr::val(common_type), PostgresType::Integer),
            ),
            Alias::new("type_key"),
        )
        .from(Alias::new(INSERTED))
        .inner_join(
            Alias::new(SEEDED),
            Expr::col((Alias::new(SEEDED), Alias::new("id"))).equals((Alias::new(INSERTED), Entities::Id)),
        )
        .to_owned();
    let members = Query::insert()
        .into_table(EntityTypeMembers::Table)
        .columns([EntityTypeMembers::EntityKey, EntityTypeMembers::EntityTypeNameKey, EntityTypeMembers::Id])
        .select_from(
            Query::select()
                .columns([Alias::new("entity_key"), Alias::new("type_key"), Alias::new("id")])
                .from(Alias::new(TYPED))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_owned();
    let typed_g = || (Alias::new(TYPED), Alias::new("g"));
    let attribute_keys = || {
        every_nth(
            typed_g(),
            1000,
            array_literal([cast(Expr::val(city), PostgresType::Integer), cast(Expr::val(alarm), PostgresType::Integer)]),
            array_literal([cast(Expr::val(city), PostgresType::Integer)]),
        )
    };
    let search = Query::insert()
        .into_table(EntitySearch::Table)
        .columns([
            EntitySearch::EntityKey,
            EntitySearch::EntityTypeNameKeys,
            EntitySearch::AttributeNameKeys,
            EntitySearch::AttributeInstanceNameKeys,
            EntitySearch::AttributeInstanceKinds,
            EntitySearch::ScopePaths,
            EntitySearch::Location,
        ])
        .select_from(
            Query::select()
                .column((Alias::new(TYPED), Alias::new("entity_key")))
                .expr(array_literal([Expr::col((Alias::new(TYPED), Alias::new("type_key")))]))
                .expr(attribute_keys())
                .expr(attribute_keys())
                .expr(every_nth(
                    typed_g(),
                    1000,
                    cast(Expr::val(vec![1_i16, 1]), PostgresType::SmallintArray),
                    cast(Expr::val(vec![1_i16]), PostgresType::SmallintArray),
                ))
                .expr(array_literal([cast(
                    text_literal("seed.").concat_text(cast(Expr::col(typed_g()).modulo(997), PostgresType::Text)),
                    PostgresType::Ltree,
                )]))
                .expr(corpus_point(&|| Expr::col(typed_g())))
                .from(Alias::new(TYPED))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_owned();
    WithClause::new()
        .cte(CommonTableExpression::new().table_name(Alias::new(SEEDED)).query(seeded).to_owned())
        .cte(CommonTableExpression::new().table_name(Alias::new(INSERTED)).query(inserted).to_owned())
        .cte(CommonTableExpression::new().table_name(Alias::new(TYPED)).query(typed).to_owned())
        .cte(CommonTableExpression::new().table_name(Alias::new("members")).query(members).to_owned())
        .to_owned()
        .query(search)
}

/// The point `(13.0 + (key % 1000) * 0.001, 52.0 + (key % 1000) * 0.001)`, in
/// WGS 84.
fn corpus_point(key: &dyn Fn() -> Expr) -> SimpleExpr {
    let offset = || Expr::expr(key().modulo(1000)).mul(0.001);
    st_point_wgs84(Expr::val(13.0).add(offset()), Expr::val(52.0).add(offset()))
}

/// The insert of one projection row per seeded Entity, `key` the column the
/// value is derived from, `value` the cell it lands in.
fn projected_corpus_insert(
    attribute: i32,
    ordinal: i32,
    attribute_kind: i16,
    value_kind: i16,
    value_column: CurrentAttributes,
    value: SimpleExpr,
    like: &str,
) -> InsertStatement {
    Query::insert()
        .into_table(CurrentAttributes::Table)
        .columns([
            CurrentAttributes::EntityKey,
            CurrentAttributes::AttributeNameKey,
            CurrentAttributes::Ordinal,
            CurrentAttributes::AttributeKind,
            CurrentAttributes::ValueKind,
            value_column,
            CurrentAttributes::CreatedAt,
            CurrentAttributes::ModifiedAt,
        ])
        .select_from(
            Query::select()
                .column((Entities::Table, Entities::EntityKey))
                .expr(Expr::val(attribute))
                .expr(Expr::val(ordinal))
                .expr(Expr::val(attribute_kind))
                .expr(Expr::val(value_kind))
                .expr(value)
                .expr(now())
                .expr(now())
                .from(Entities::Table)
                .and_where(Expr::col((Entities::Table, Entities::Id)).like(like))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_owned()
}

/// Write the corpus and analyze the relations the plans read.
///
/// Every Entity lands in the current-state relations: the identity row and a
/// search row carrying the type, the attribute keys (the city attribute, and
/// on every 1000th Entity the alarm attribute), the location and the scope.
/// `current_attributes` receives a numeric reading, a `GeoProperty` entrance,
/// the city string and the owner Relationship per Entity.
pub async fn seed(pool: &PgPool, prefix: &str) -> SeededCorpus {
    let reading = attribute_name_key(pool, READING_ATTRIBUTE)
        .await
        .expect("attribute dictionary seed must succeed");
    let entrance = attribute_name_key(pool, ENTRANCE_ATTRIBUTE)
        .await
        .expect("attribute dictionary seed must succeed");
    let city = attribute_name_key(pool, CITY_ATTRIBUTE).await.expect("attribute dictionary seed must succeed");
    let alarm = attribute_name_key(pool, ALARM_ATTRIBUTE).await.expect("attribute dictionary seed must succeed");
    let owner = attribute_name_key(pool, OWNER_ATTRIBUTE).await.expect("attribute dictionary seed must succeed");
    let rare_type = entity_type_name_key(pool, RARE_TYPE).await.expect("type dictionary seed must succeed");
    let common_type = entity_type_name_key(pool, COMMON_TYPE).await.expect("type dictionary seed must succeed");

    entity_corpus(prefix, rare_type, common_type, SEEDED_ENTITIES, city, alarm)
        .to_query()
        .execute(pool)
        .await
        .expect("entity corpus must be writable");

    let like = format!("{prefix}:seed:%");
    let entity_key = || Expr::col((Entities::Table, Entities::EntityKey));
    projected_corpus_insert(
        reading,
        0,
        1,
        1,
        CurrentAttributes::ValueNumber,
        cast(Expr::expr(entity_key().modulo(20_000)), PostgresType::Numeric),
        &like,
    )
    .to_query()
    .execute(pool)
    .await
    .expect("numeric projection corpus must be writable");

    projected_corpus_insert(entrance, 1, 3, 4, CurrentAttributes::ValueGeometry, corpus_point(&entity_key), &like)
        .to_query()
        .execute(pool)
        .await
        .expect("geometry projection corpus must be writable");

    projected_corpus_insert(
        city,
        2,
        1,
        2,
        CurrentAttributes::ValueString,
        text_literal("city-").concat_text(cast(Expr::expr(entity_key().modulo(1000)), PostgresType::Text)),
        &like,
    )
    .to_query()
    .execute(pool)
    .await
    .expect("string projection corpus must be writable");

    projected_corpus_insert(
        owner,
        3,
        2,
        7,
        CurrentAttributes::ValueUri,
        cast(Expr::val(OWNER_PREFIX), PostgresType::Text).concat_text(cast(Expr::expr(entity_key().modulo(2000)), PostgresType::Text)),
        &like,
    )
    .to_query()
    .execute(pool)
    .await
    .expect("relationship projection corpus must be writable");

    analyze(pool).await;

    SeededCorpus {
        prefix: prefix.to_owned(),
        reading,
        entrance,
        rare_type,
        common_type,
        city,
        alarm,
        owner,
    }
}

/// Project one high-entropy essay past the indexed bound onto each of the
/// first [`SEEDED_ESSAYS`] Entities of a seeded corpus, and re-analyze.
///
/// The essays land in `value_long_text` under value kind 2, the placement the
/// writer gives every String longer than 2048 UTF-8 octets, so the overflow
/// indexes hold exactly these rows.
pub async fn seed_essays(pool: &PgPool, corpus: &SeededCorpus) -> SeededEssays {
    let attribute = attribute_name_key(pool, ESSAY_ATTRIBUTE).await.expect("attribute dictionary seed must succeed");
    let essays: Vec<String> = (0..SEEDED_ESSAYS).map(|seed| high_entropy_text(ESSAY_LENGTH, seed)).collect();
    let probe = essays
        .get(PROBED_ESSAY)
        .and_then(|essay| essay.get(ESSAY_LENGTH / 2..ESSAY_LENGTH / 2 + PROBE_LENGTH))
        .expect("the probed essay is seeded and ASCII")
        .to_owned();

    let bodies = [UnnestColumn::new(Alias::new("body"), PostgresType::TextArray)];
    let essay = |column: &str| Expr::col((Alias::new("essay"), Alias::new(column)));
    let insert = Query::insert()
        .into_table(CurrentAttributes::Table)
        .columns([
            CurrentAttributes::EntityKey,
            CurrentAttributes::AttributeNameKey,
            CurrentAttributes::Ordinal,
            CurrentAttributes::AttributeKind,
            CurrentAttributes::ValueKind,
            CurrentAttributes::ValueLongText,
            CurrentAttributes::CreatedAt,
            CurrentAttributes::ModifiedAt,
        ])
        .select_from(
            Query::select()
                .column((Entities::Table, Entities::EntityKey))
                .expr(Expr::val(attribute))
                .expr(Expr::val(4))
                .expr(Expr::val(1_i16))
                .expr(Expr::val(2_i16))
                .expr(essay("body"))
                .expr(now())
                .expr(now())
                .from_function(unnest_call([(&bodies[0], Expr::val(essays))]), Alias::new("essay"))
                .inner_join(
                    Entities::Table,
                    Expr::col((Entities::Table, Entities::Id)).eq(cast(Expr::val(corpus.prefix()), PostgresType::Text)
                        .concat_text(text_literal(":seed:"))
                        .concat_text(lpad(cast(essay("ordinal"), PostgresType::Text), Expr::val(6), text_literal("0")))),
                )
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_owned();
    UnnestAliased::new(insert)
        .with_ordinality(Alias::new("essay"), &bodies, Alias::new("ordinal"))
        .to_query()
        .execute(pool)
        .await
        .expect("overflow projection corpus must be writable");

    analyze(pool).await;

    SeededEssays { attribute, probe }
}

/// Refresh the statistics the planner reads for every relation the plans touch.
pub async fn analyze(pool: &PgPool) {
    for relation in [
        Entities::Table.into_iden(),
        EntitySearch::Table.into_iden(),
        CurrentAttributes::Table.into_iden(),
    ] {
        analyze_relation(MaintenanceSchema::Public, relation)
            .to_query()
            .execute(pool)
            .await
            .expect("analyze must succeed");
    }
}

/// The planner paths a rolled-back `EXPLAIN` leaves open.
#[derive(Clone, Copy)]
pub enum PlannerPaths {
    /// Every path the planner considers by default.
    Unrestricted,
    /// Sequential scans priced out, for a corpus too small for the planner to
    /// prefer an index on cost alone: the plan then shows whether the index can
    /// answer the predicate at all.
    WithoutSequentialScans,
}

/// Run `EXPLAIN (ANALYZE, BUFFERS)` on one executor and return the plan as one
/// text block.
pub async fn explain<'e, E>(executor: E, statement: impl Statement) -> String
where
    E: Executor<'e, Database = Postgres>,
{
    let lines: Vec<String> = Explain::new(statement)
        .with(ExplainOption::Analyze)
        .with(ExplainOption::Buffers)
        .to_query_scalar()
        .fetch_all(executor)
        .await
        .expect("the plan must be readable");
    lines.join("\n")
}

/// Run `EXPLAIN (ANALYZE, BUFFERS)` inside a transaction that is rolled back
/// afterwards, so a planner setting stays local to the one statement and a
/// planned write leaves nothing behind.
pub async fn explain_rolled_back(pool: &PgPool, paths: PlannerPaths, statement: impl Statement) -> String {
    let mut transaction = pool.begin().await.expect("the plan transaction must open");
    match paths {
        PlannerPaths::Unrestricted => (),
        PlannerPaths::WithoutSequentialScans => {
            SettingAssignment::new(Setting::EnableSeqscan, "off", SettingScope::Transaction)
                .to_query()
                .execute(&mut *transaction)
                .await
                .expect("the planner setting must apply");
        }
    }
    let plan = explain(&mut *transaction, statement).await;
    transaction.rollback().await.expect("the plan transaction must roll back");
    plan
}
