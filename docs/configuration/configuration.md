# Configuration

The broker builds its configuration from four layers, and each later layer overrides the one before it. Every key but the database connection has a built-in default, so the broker needs no file, only `database.connection`, which the environment can give. This page is for people who change the configuration itself. The operator reference is built from the descriptor described below.

## Files in this directory

Three files here are generated. Do not edit them by hand. Regenerate them with the command shown in [Changing a key](#changing-a-key).

- [`andromeda.toml`](andromeda.toml) is the file `andromeda configuration generate` writes. It lists every table and key in file order, each at its default, under its description and the limits the validator applies. A key that is not set by default is shown commented out, at an example value. This is the in-repository key reference.
- [`andromeda.schema.json`](andromeda.schema.json) is the JSON Schema that `andromeda configuration schema` writes. It is draft 7. `.taplo.toml` validates every `andromeda.toml` in the repository against it, and each release publishes it. The schema checks names, types and choices. It requires no key, because the loader lays the file over the defaults.
- [`andromeda.descriptor.json`](andromeda.descriptor.json) is the file `andromeda configuration describe` writes, and each release publishes it too. For every key it gives the path, the description, the type name (such as `non-zero duration`), the default or example, the allowed choices and what each one means, the environment variable, whether a change needs a restart, whether replicas must agree on the value, whether the value is sensitive, and every limit and condition the validator applies to it. Its `rules` list holds every rule that is not a single comparison as data: the keys it names, its statement, the conditions under which it applies (`when`, all of them at once) and what it then asks (`then`, every one of them). A tool evaluates `rules` and each key's `limits` to apply the rules the validator applies. A documentation tool renders the descriptor. Andromeda itself generates no Markdown.

## Where the configuration is defined

Everything lives in `andromeda-configuration`.

- `schema/` holds one type per table. Every type, field and enum variant carries `#[schemars(description = "...")]`. That text is what the operator reads, and it stays apart from the rustdoc. A `PathBuf` field carries `title = "path"`. The `configuration_is_described_for_the_operator` architecture check rejects any item that has no description.
- `validate/` holds the rules, every one of them as data that the validator runs and the descriptor lists. A rule that compares one key with zero, a fixed limit or another key is a `Rule`. Any other rule is a `CompoundRule`: its conditions (a key at one of some keywords, a key at any keyword but one, a listener that terminates TLS), its obligations (a key set or left out, a list empty or not, a key not at one keyword, two keys different, list entries unique, a byte size that fits the address width, a `Rule`), and one sentence stating it. A rule that needs a condition or an obligation of a new kind adds the kind to `compound_rule.rs`, its evaluation to `compound_check.rs` and its descriptor shape to `reference/rule_descriptor.rs`.
- `reference/` builds the three generated files from the schema types, the declared rules and the example configuration. The example gives a value to every key that is not set by default, so the template can show it.

## Changing a key

1. Change the type in `schema/` and write its description. Then choose how the key behaves.
   - A key that a running broker picks up without a restart takes `#[schemars(extend("x-andromeda-apply" = "reload"))]`.
   - A key that every replica must hold the same way takes `#[schemars(extend("x-andromeda-replica-consistent" = true))]`.
   - A key whose value is a secret goes into `SENSITIVE_KEYS` in `redact.rs`. The descriptor reads that list to mark the key sensitive. `configuration show` does not read it. It redacts the two database connections inside `redacted()` in the same file, so a new secret key needs a change there as well.
   - A key with none of these markers restarts the broker to take effect, may hold any value on each replica, and is not sensitive.
2. Add or change its rules in `validate/`, with a boundary test.
3. Regenerate the three files.

   ```bash
   ANDROMEDA_REGENERATE=1 cargo test -p andromeda-configuration
   ```

4. Check in the result. If step 3 is skipped, the crate's tests fail. The failure names the file that differs and its first differing line.

There are no aliases for old keys. The loader reads a key only at its current path, so a renamed key has to change everywhere: compose files, harness stacks and the documentation site. A file that still uses the old name loads with a warning for that key, and the setting takes its default.

## Loading

The layers, in increasing precedence:

1. The built-in defaults.
2. The file named by `-c` or `--configuration` (`--config` also works). Without the flag, `andromeda/andromeda.toml` under the platform configuration directory is read if it exists. An explicit path must exist, or loading fails.
3. `ANDROMEDA__` environment variables. The key path goes in upper case, with `__` between levels, as in `ANDROMEDA__DATABASE__CONNECTION`. A single underscore joins the words of one key, so `ANDROMEDA__NOTIFICATIONS__PERIODIC__MAX_POLL_INTERVAL` sets `notifications.periodic.max_poll_interval`. A variable sets one scalar value. A list takes its members separated by commas. A table can be set only in the file, with one exception: a database connection is either a URL or a table of its parts, and each part can be its own variable, such as `ANDROMEDA__DATABASE__CONNECTION__PASSWORD`. A connection part other than `port` is read as text, so a password made of digits keeps its leading zeros.
4. `--host` (`-H`) and `--port` (`-p`).

### Keys the configuration does not define

A key the configuration does not define is ignored, whether it appears in the file or in an `ANDROMEDA__` variable. A file written for a later version therefore still loads. At startup the broker logs one warning for each ignored key, with the event `configuration_setting_ignored`, the key's path and the layer that writes it. Paths look like `server.unknown_key` or `jsonld.preloaded_contexts[1].unknown_key`. An unknown table is reported once, as a whole.

A key the configuration does define stays strict. A value of the wrong type, a malformed duration or size, or a choice the key does not offer is refused, because the broker cannot tell what the value means. The JSON Schema lists only the keys the configuration defines, so an editor flags a misspelt key while the file is being written.

The file carries no version key, and the loader translates nothing between versions. A file whose keys an older build does not know loads with warnings. It never loads with a different meaning. A migration tool may come later.

## Commands

- `andromeda configuration validate` runs the full load and every rule. It prints each broken rule on its own line and a warning for each ignored key. Ignored keys alone do not make it fail.
- `andromeda configuration show` prints the effective configuration with the database connections redacted. It prints TOML by default and JSON with `-f json`. Warnings go to standard error.
- `andromeda configuration generate`, `schema` and `describe` print their document to standard output, or write it to a file with `-o`.
