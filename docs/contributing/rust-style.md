# Rust style

Many of these rules are workspace lints in `Cargo.toml` or gates in `andromeda-architecture-checks`. Each one names its lint or gate, so a failing `cargo clippy` or test leads back here. The rest are checked in review.

Every crate takes its lint levels from `[workspace.lints]`, so a rule never holds in one crate and lapses in the next. Clippy's pedantic group is on as warnings, and CI denies warnings, so those hold too. A lint is silenced only with `#[expect(..., reason = "...")]`, never `#[allow]` (`clippy::allow_attributes`), and an expectation that no longer fires is a warning, which fails clippy in CI, so a suppression can't outlive the code that needed it.

## Formatting

Format with `cargo fmt`. `rustfmt.toml` turns on unstable options (`unstable_features`, `imports_layout`, `imports_granularity`), so only a nightly rustfmt formats the code correctly, and `cargo fmt` runs the nightly that `rust-toolchain.toml` pins.

## Imports

- All `use` statements sit at the top of the file in one contiguous block, with no blank lines or groups, so the formatter can sort and deduplicate them.
- Import with `crate::` paths rather than `super::`.
- A path of three or more segments is never written inline. Import it and use the short name. Two-segment paths such as `url::Url`, `serde_json::Value` or `std::fmt` are fine inline, and the rule applies everywhere, in bodies, field types, signatures, trait bounds and where clauses (`clippy::absolute_paths`).
- Glob imports are not allowed (`clippy::wildcard_imports`).

## Matching

- Never match an enum with `_ => {}` or another wildcard catch-all. List every variant, and group variants that share their handling with `|` (`clippy::wildcard_enum_match_arm`).
- For types with no fixed set of values, such as integers and strings, prefer a named binding like `other => ...` to `_`.

## Errors

- Production code never calls `unwrap()` or `expect()` and never panics. Propagate with `?`, use a combinator or match the value (`clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`, together with `todo`, `unimplemented` and `unreachable`). `clippy.toml` disallows `sqlx::Row::get` and `sqlx::Row::get_unchecked` for the same reason, so decode a column with `try_get` and propagate the failure.
- Test code may unwrap, expect and panic, since `clippy.toml` allows it in `#[test]` functions and `#[cfg(test)]` modules. Code behind a `test-support` feature counts as production code.
- Each module or domain defines typed error enums with `thiserror`. A type-erased error is not allowed. The gate `type_erased_error` refuses a `dyn Error` trait object such as `Box<dyn Error>`, and every crate's dependency contract refuses `anyhow`.
- An error variant carries typed fields and chains its cause through `#[source]` or `#[from]`. It never holds a message built with `format!` or a source error flattened with `to_string()` (gate `string_typed_error`). A cause that can take several shapes gets an enum of its own, one variant per shape, so a caller can still tell one fault from another and the source stays attached until it's logged.
- A `Result` is never ignored. A row decode failure in particular is propagated, never swallowed (gate `swallowed_row_decode`).
- The `detail` of a ProblemDetails response is the `Display` of a typed error, written next to the variant. The HTTP layer only picks the problem type and status for the variant and passes the message through. It never builds a `format!` string of its own, so two endpoints that hit the same fault say the same thing.
- A message says what is wrong, names the member, parameter or value it's about, and says what was expected. It uses the client's vocabulary (`id` and `type`, not `@id` and `@type`) and quotes values the way the client sent them. It cites no clause numbers and no internal names. The prose starts in lowercase unless it opens with a value, a specification token or a name that is spelt with capitals, such as `GeoJSON` or `TLS`, and no value is interpolated with `Debug` (gate `detail_message`). Tests match on error variants, not on message text.
- A fault of the broker's own, such as a stored row that doesn't decode or a broken invariant, is logged once with its full cause. The client gets `InternalError` with a short summary such as `internal registration error`, never the cause, which could reveal hostnames or SQL. The broker never reports its own fault as a problem with the request. A database outage, for example, is a `503` `ServiceUnavailable` with `Retry-After`.
- A stored entity the broker can no longer decode is a fault of that entity alone. A read that covers many entities logs it and goes on without it. Query Entities, for example, reports it with an `NGSILD-Warning` of code `299` that names it. A read of that entity by its id, and every write that starts from its state, fails with `InternalError`.

## Constants

The lints that ban `unwrap`, `expect` and `panic` apply inside `LazyLock` and other one-time initializers too. So a constant whose type has only a fallible constructor is validated at compile time instead. Use `url_macro::url!` for URLs, `iri!` for IRIs, and an infallible `from_static(&'static str)` constructor in the owning crate for string newtypes. An IRI or media type is never kept as a plain string constant (gate `raw_constant`).

## Casts and safety

- `unsafe` code is forbidden (`unsafe_code`).
- A numeric cast that can truncate, wrap or lose its sign is denied (`clippy::cast_possible_truncation`, `clippy::cast_possible_wrap`, `clippy::cast_sign_loss`). Convert with `TryFrom` and handle the failure.

## Idioms

- Prefer iterator chains to index loops. Reach for `any`, `all`, `find`, `position` or `sum` before a hand-written loop, and only `collect` when you need the collection itself (`clippy::needless_collect`).
- Prefer `?` to matching on a `Result` by hand, and pattern matching to checking flags.
- Wrap bare primitives in newtypes, and shape types so that invalid states can't be represented.
- Avoid `.clone()`. Borrow first, or use `Cow` or `Arc`. When a clone is really needed, a comment says why (`clippy::redundant_clone`).
- Write explicit lifetimes when a function returns a reference derived from several inputs, when a struct holds references, or when the borrowing isn't obvious.
- When a binding only needs to be mutable while it is built, shadow it as immutable afterwards.

  ```rust
  let mut data = get_vec();
  data.sort();
  let data = data;
  ```

- A function that needs no runtime values is a `const fn` (`clippy::missing_const_for_fn`).
- `new()` returns the concrete type name rather than `Self`, unless a trait requires `Self`.
- A conversion that can fail is a `TryFrom`. A `From` impl succeeds for every input.
- Keep drop order in mind. Fields drop in declaration order and locals in reverse, and cleanup logic belongs in an explicit `Drop` impl.
- An item that nothing outside its crate uses is not `pub` (`unreachable_pub`), and a path is not qualified more than it needs to be (`unused_qualifications`).

## Booleans

A `bool` parameter or return value becomes a two-variant enum, such as `enum Visibility { Public, Private }`, unless the concept really is boolean, as in `is_empty()`. A struct with more than three bools becomes a typed set, `enumset::EnumSet<SomeEnum>`.

## Concurrency

- Use `DashMap` for a concurrent map, never `Mutex<HashMap>` or `RwLock<HashMap>`.
- Use `RwLock` rather than `Mutex` for state that is mostly read.
- Use `IndexMap` when iteration order matters.
- The workspace denies a lock guard held across an `.await` (`clippy::await_holding_lock`), an `async fn` whose future isn't `Send` (`clippy::future_not_send`), and an `Arc` around a type that can't cross threads (`clippy::arc_with_non_send_sync`). Each would deadlock the runtime or keep a task from moving between worker threads.

## Macros

Write a `macro_rules!` macro when the same code shape repeats three or more times with small variations, and for table-driven tests. Don't write one when a generic function, a trait or an iterator chain says the same thing more clearly. Procedural macros live in a dedicated `*-derive` or `*-macros` crate. A macro is documented like a public function and is hygienic, so it captures no identifier from the call site unless the identifier is passed in. Check its expansion with `cargo expand` (installed with `cargo install cargo-expand`) for hidden clones, unwraps or panics.
