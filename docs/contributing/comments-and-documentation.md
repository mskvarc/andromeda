# Comments and documentation

## Comments

A comment says what the code cannot. That means why the code is shaped the way it is, an invariant, a constraint, the specification clause behind it, a non-obvious edge case, or a workaround and its reason. A comment that repeats the next line in English is deleted, not shortened. So are step narration, section banners and dividers, Given/When/Then or Arrange/Act/Assert markers, commented-out code, and a TODO or FIXME that gives no reasoning.

A comment states what is true now and never tells the history of the code. Words like "previously", "used to", "formerly", "no longer", "before the fix", "pre-fix", "the old X", "was changed to", "replaced", "retired", "legacy", "temporary", "for now", "regressed", "the defect", "the fix", "bug where" and "an earlier version" give a history comment away. A past bug becomes an invariant stated in the present tense, or the comment goes.

```rust
// Fixed the bug where mapped addresses bypassed the check.
```

becomes

```rust
// ::ffff:127.0.0.1 is classified as the IPv4 address it embeds.
```

A comment stands alone. It never points at another file, path, module, migration, issue tracker, pull request, other broker, external test suite or internal plan id. The only references allowed are published standards cited by title and clause, such as "ETSI GS CIM 009 v1.9.1 clause 5.6.1" or "RFC 8288 section 3", and only where the clause justifies the behaviour.

Write the way a terse engineer would. Don't open with filler such as "This function..." or "Ensures that...", and leave out promotional words and hedging. Don't use em or en dashes for asides. A comma, parentheses or a new sentence works instead.

In tests, the test name carries the scenario. A test comment is allowed only to justify an expected value or a non-obvious ordering or concurrency constraint. How to run the tests is documented separately and never goes in a test comment.

The header of a SQL migration is the one exception to the banner rule. It opens with `-- ====` rule lines around a `-- NNNN: Title` line that names the file's own version, and `andromeda-sql lint` checks that shape. Every other comment in a migration follows the rules above. `comment-style` in [SQL lint rules](../database/sql-lint-rules.md) describes the rest of the format.

## Rustdoc

The workspace denies `missing_docs` and `clippy::missing_docs_in_private_items`, so every item, field and variant has a doc comment. A doc that only restates the name gets real information added (semantics, units, what `None` means, error conditions, invariants) and is never deleted. A module's `//!` says what the module owns and why. Docs follow the comment rules above.

## Longer explanations

Design rationale, protocols that span modules, interpretations of the specification, operational runbooks and test recipes belong in the documentation, not in comments. When you remove a comment that carried such content, move the content into the documentation instead of dropping it. Write each Markdown paragraph as a single line, with no hard line breaks inside it.
