# Commits and pull requests

Pull requests are merged with rebase merge, so every commit lands on `main` as it stands and the history stays linear. Merge commits are turned off. Squash merge is allowed only for a branch whose commits can't be cleaned up.

## Commits

Every commit starts with a [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) header.

```
<type>(<scope>)?: <summary>
```

- The type is one of `feat`, `fix`, `perf`, `refactor`, `test`, `docs`, `build`, `ci` or `chore`.
- The scope is optional. When you use one, name the part of the broker it touches, such as `temporal`, `federation`, `query` or `config`, without the `andromeda-` crate prefix.
- The summary is imperative and lower case, with no final period. It says what changes, and for a change in behaviour, what the broker does afterwards. Write `fix: refuse PUT and PATCH on the entity collection with 405`, not `fix: update handler`.
- A breaking change adds `!` after the type or scope, and a `BREAKING CHANGE:` footer says what users or operators have to change.
- The body explains why, when the header can't.

The issue goes on a line of its own, never in the header. Write `Closes #N` when the commit finishes the issue and `Refs #N` when it is partial or follow-up work.

Each commit is one coherent change that compiles, passes clippy without warnings and passes the tests on its own, because `git bisect` lands on single commits. Use `git rebase -i` to fix up, split and reorder commits before review, and force-push with `--force-with-lease`.

## In the changelog

The changelog and the release notes are built from commit headers and footers, with the summary as written, so every commit is also a changelog entry.

- `feat`, `fix` and `perf` commits are listed for users. Every other type is an internal change, listed only for the newest release. A change users can see takes `feat` or `fix`, whatever files it touches.
- The `security` scope, as in `fix(security): ...`, files a commit under Security. Use it for the fix to a vulnerability.
- A summary that starts with `deprecate` files the commit under Deprecations.
- A breaking commit is listed under Breaking changes, with its summary followed by its `BREAKING CHANGE:` footer. Write the footer as sentences an operator can act on.
- `Closes #N` and `Refs #N` become links to the issues. Each goes on its own line in the last paragraph of the message.
- Put `@` words such as `@context` in backticks, because GitHub links a bare one to the account of that name.

## Pull request title

The title is a Conventional Commits header for the whole pull request. With a single commit, it is that commit's header. With several, take the type of the main change (a `fix` with its tests is a `fix`) and summarise the outcome rather than the steps. Mark a breaking change with `!`. Issue numbers go in the body, not the title.

## Pull request body

The template starts with one `Closes #N` or `Refs #N` line per issue, so a pull request can carry a series of issues. Three sections follow.

- **Change** says in a few words what the broker does differently afterwards.
- **Cause** says why, citing the ETSI GS CIM 009 v1.9.1 clause when the specification decides the behaviour. Delete it when the issue already explains this.
- **Verification** lists what CI doesn't show, such as checks run outside CI, benchmark results, failures that also happen on `main` and anything that wasn't run. Delete it when CI covers the change.

Pull requests carry no labels, since their commit types already say what they are.

## Review

Keep the branch rebased on `main`, because GitHub can't rebase a branch that has conflicts. During review, address feedback with new fixup commits if that helps the reviewer, then squash them into the commits they belong to before the merge.
