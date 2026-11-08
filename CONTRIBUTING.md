# Contributing to Andromeda

Andromeda is an NGSI-LD context broker written in Rust. It implements ETSI GS CIM 009 v1.9.1 from scratch as an alternative to the JVM-based brokers, and it can serve as a context source in FIWARE deployments and data spaces. Correctness and conformance to the specification come first, then a strongly typed domain model, then performance, which comes from few allocations and async I/O throughout.

## The Specification Decides

ETSI GS CIM 009 v1.9.1 is the source of truth for every API behaviour, data model, error response and semantic rule. Read the relevant clause before you implement or change NGSI-LD behaviour, and cite it by title and clause when you justify a decision, as in "ETSI GS CIM 009 v1.9.1 clause 5.6.1". The specification wins over assumptions, convenience and whatever other brokers do. Where it leaves a choice open, the interpretation Andromeda takes is written down.

## Before You Start

Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).

- **Security vulnerabilities** never go in a public issue. Follow the [Security Policy](SECURITY.md) instead.
- **Questions** go to [Discussions](../../discussions).
- **Everything else** starts as an [issue](../../issues), as described below. Small fixes, such as a typo, can go straight to a pull request.

## Opening an Issue

Blank issues are turned off, so every issue starts from a form.

If you just want to report something, use one of the simple forms, **Report a problem** or **Suggest an idea**. **Note** is a simple form too, for something to look into later. You don't need to know what kind of issue it is or what a fix would have to prove. They only ask for what you can know without reading the code, and triage works out the rest.

If you know the kind, you're welcome to use its own form instead. There's one for a bug, a conformance deviation, a performance problem, a feature, a documentation correction, a decision, a research question, a refactor, a build change and a test suite gap. These ask for more, usually including what has to hold once the work is done and how to show it, so an issue filed through one is close to ready to work on, and a pull request can be checked against it. [Issue triage](docs/issue-triage.md) describes what happens to an issue after you file it.

## Workflow

1. Set up the toolchain and run the checks, as [Getting started](docs/contributing/getting-started.md) describes.
2. Branch from `main` and name the branch `<type>/<issue>-<slug>`, for example `fix/422-collection-put-405`.
3. Make the change following the rules below. Every rule applies to every file you touch.
4. Write your commits and open the pull request as [Commits and pull requests](docs/contributing/commits-and-pull-requests.md) describes.

## Rules

| Topic | Covers |
|---|---|
| [Architecture](docs/contributing/architecture.md) | Layering, crate and module ownership, file size, configuration ownership, no shims |
| [Rust style](docs/contributing/rust-style.md) | Formatting, imports, matching, error handling, idioms |
| [Type design](docs/contributing/type-design.md) | Domain value types, constants, JSON, derives, serialization casing, HTTP headers |
| [Comments and documentation](docs/contributing/comments-and-documentation.md) | What a comment may say, rustdoc, where design rationale goes, Markdown rules |
| [Dependencies](docs/contributing/dependencies.md) | Choosing crates, declaring them, package metadata, code from other projects |
| [Database migrations](docs/contributing/database-migrations.md) | Migration format, formatting, linting, sealing |
| [Testing](docs/contributing/testing.md) | Unit tests, the black-box test suite, what a fix must prove |

The architecture checks (`andromeda-architecture-checks`) and clippy enforce many of these rules automatically. Review catches the rest.

## General

- Leave no compiler or clippy warnings, including ones that were there before your change. The workspace denies `#[allow(...)]` outright. Silencing a lint with `#[expect(..., reason = "...")]` needs the maintainers' agreement first, so explain the warning and the fix you propose.
- Never relax a lint to get a change through. Don't raise a threshold in `clippy.toml` or loosen `[lints]`. Change the code instead.
- YAML files use `.yaml`, never `.yml`.
- File and directory names are lowercase kebab-case, except Rust modules and the files GitHub expects by name.
- A new check is a script in the directory that owns it, and it gets documented. It never becomes a new job in the integration workflow.
- No emojis in code, comments, documentation, commit messages or pull requests.
- Name things after what they are, such as the broker, the server, the runtime or the tenant registry. Generic names like `App` or "the app" say nothing about the part they refer to.

## Licence

By contributing, you agree that your contribution is licensed under the [European Union Public Licence v. 1.2](LICENSE.md).
