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
