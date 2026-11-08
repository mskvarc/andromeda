# Issue triage

An issue with no milestone is untriaged. Triage either turns it into a work item or closes it, so every open issue with a milestone has a kind, an area, and a body someone can work from. [Contributing](../CONTRIBUTING.md#opening-an-issue) describes the forms an issue is filed through.

## Kinds

Each advanced form stands for one kind of work item and adds its label. The simple forms, Report a problem and Suggest an idea, and the Note form add no label.

| Form | Label | Kind |
|---|---|---|
| Bug report | `bug` | Broken on its own terms, with no specification clause at stake. |
| Conformance report | `conformance` | Behaviour contradicts ETSI GS CIM 009 v1.9.1. |
| Performance report | `performance` | Correct, but too slow or too large. |
| Feature request | `feature` | A capability the broker lacks, from the specification or as an extension. |
| Documentation correction | `docs` | User or operator documentation is wrong or missing. |
| Decision request | `decision` | A question about what the broker should do, which needs a ruling. |
| Research question | `research` | A measurement to take, a cause to find or an option to investigate. |
| Refactor proposal | `refactor` | The structure of the code changes and its behaviour doesn't. |
| Build change | `build` | Images, Compose files, CI, release gates or developer tooling. |
| Test suite gap | `testing` | A gap or defect in the test suite, its stacks or its fixtures. |

## Triaging an issue

If the issue is accepted, it gets its kind label if it doesn't have one yet, one `area:` label and a milestone. An issue filed through a simple form or as a note has its body rewritten under the headings of its kind's form. The reporter's facts (the request, the response, the version, the logs) move under the matching headings, and triage writes the sections only a maintainer can, such as Acceptance and Proof. An issue filed through an advanced form keeps its body, and triage fills in whatever is missing.

If it's closed as not planned, it's a duplicate, a request outside what the broker does, or a report that can't be reproduced and got no answer when more detail was asked for. The closing comment says which, and links the original issue for a duplicate.

A question filed as an issue is converted to a discussion.
