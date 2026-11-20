# Database migrations

The migrations under `migrations/` are the schema every release ships. They come in three sets, `platform/`, `tenant/` and `branch/`, which share one version space. [Schema](../database/schema.md#migrations) describes where each set runs. `tools/andromeda-sql` formats, lints, seals and verifies them. [SQL tooling](../database/sql-tooling.md) describes the tool, [SQL lint rules](../database/sql-lint-rules.md) its rules, and [Migration sealing](../database/migration-sealing.md) the seal.

## Rules

- A migration that a stable release has sealed never changes. The broker stores each applied migration's checksum and refuses a database where an applied migration was edited or removed. Change a sealed schema with a new migration.
- A migration that no stable release has sealed is still being written. Edit it in place rather than adding a migration that alters it.
- A new migration takes the next free version, whichever set it belongs to.
- Format, lint and verify every migration you add or change.

  ```sh
  cargo run -p andromeda-sql -- format
  cargo run -p andromeda-sql -- lint
  cargo run -p andromeda-sql -- verify --against origin/main
  ```

- Comments in a migration follow [Comments and documentation](comments-and-documentation.md). The header every migration opens with is described under `comment-style` in [SQL lint rules](../database/sql-lint-rules.md).
- In Rust code, SQL is spelled only in `andromeda-postgresql-seaquery` and never in another crate. The gate `raw_sql` enforces this, and exempts only its own detectors.
- In the Change section of the pull request, say what the migration means for an operator, such as its lock behaviour, its run time on large tables and anything they have to do.

## Functions

A migration creates a function only when its statements need one, and every function follows the same shape.

- The clauses after the parameters come in one order: `returns`, `language`, the volatility, `strict` when it applies, `parallel`, then the body. Every function but a trigger function states its volatility and its parallel safety, which the `function-attributes` rule enforces.
- `strict` is declared only where the body would turn a NULL argument into a result that must stay NULL, such as an `array (...)` that would make an absent array empty. A function a query inlines is never `strict`, because PostgreSQL inlines a strict SQL function only when its body is strict too.
- A function is written in SQL with a SQL-standard body (`return ...` or `begin atomic ... end`) wherever SQL can say what it does. PL/pgSQL is for a loop whose steps depend on each other, for replacing fields of a composite value, and for a parameter of a polymorphic type, which a SQL-standard body cannot take. The comment above a PL/pgSQL function says which of these it needs.
- A pure function is named for what it returns, a predicate reads as one, and a function with side effects, a trigger function included, is named for what it does.
- A helper that only its own migration calls is created in `pg_temp` and dropped by that migration's last statement. The migrator runs every migration of a run in one session, so a temporary function that stays would reach every later migration and every Snapshot branch the run migrates.
- A helper that another migration set calls, as the branch set calls the functions that convert data-integrity seals, lives in a schema named for its purpose, never in `public`, and its calls name that schema. The migration that ends its use drops each function by name and then the schema without `cascade`, so a function left out of the list fails the migration instead of outliving its purpose. A branch migration that calls such a function first returns when the branch holds no row it would convert, because a Snapshot created after the schema is dropped runs the branch migrations over empty relations.
- The `tests/schema` target of `andromeda-sql` lists every function the migrations leave behind, and checks that every schema a migration creates is dropped again.

A migration's `Codes:` block lists every code the file reads or writes, with the column or composite field it belongs to, not only the codes the file introduces.
