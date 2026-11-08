# Security Policy

## Reporting a Vulnerability

Please don't report security issues in public issues, pull requests or discussions.

Use [GitHub private vulnerability reporting](../../security/advisories/new) instead. Only the maintainers can see the report, and it stays private until a release with the fix is out.

Include as much of the following as you can.

- the Andromeda version or commit you run
- how it is deployed, meaning the container image, the PostgreSQL version and anything in front of the broker
- what the issue is and what an attacker can do with it
- the exact requests or payloads that reproduce it

## Supported Versions

Fixes go into the latest release.

## Scope

Andromeda has no authentication or authorization of its own. Deciding who can reach the broker is up to the deployment, and the broker should only be exposed to callers the operator trusts. An endpoint that answers an unauthenticated request is therefore not a vulnerability. What a caller can do once they reach the broker is.

These findings are the most important to report.

- A way past the outbound address checks. When the broker is set to refuse private and reserved addresses, it checks every notification endpoint, context source and `@context` download against the address the host name actually resolves to. Anything that still lets it reach the operator's internal network is in scope. The default settings allow these addresses, so reaching them with the defaults is not a finding.
- A break in tenant isolation, where one tenant's entities, temporal history or schema can be read or written through another tenant's requests, snapshot routes or branch routes.
- SQL injection.
- Denial of service through crafted entity, subscription or registration payloads.
- Flaws in the broker's TLS termination.

CI checks every dependency against known advisories with cargo-deny. A vulnerable crate needs a report only if it can be exploited through the broker. One with no reachable path is fixed through the normal dependency updates.

## Handling

You'll hear back within a few days, and the investigation stays private. The advisory credits you by default, so say so in the report if you'd rather stay anonymous. It goes public once a release with the fix is published.
