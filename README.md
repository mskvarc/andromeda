<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/andromeda-dark.svg">
    <img src="docs/assets/andromeda-light.svg" alt="" width="280">
  </picture>
</p>

<h1 align="center">Andromeda</h1>

<p align="center">
  <strong>Context you can trust.</strong><br>
  A strict <a href="https://ngsi-ld.org/">NGSI-LD</a> context broker written in Rust, with the full v1.9.1 API.
</p>

<p align="center">
  <a href="#where-it-fits">Where it fits</a> ·
  <a href="#why-andromeda">Why Andromeda</a> ·
  <a href="#try-it">Try it</a> ·
  <a href="#roadmap">Roadmap</a> ·
  <a href="#community">Community</a> ·
  <a href="#licence">Licence</a> ·
  <a href="https://andromeda.skvarc.com">Documentation</a>
</p>

<p align="center">
  <a href="LICENSE.md"><img alt="license: EUPL-1.2" src="https://img.shields.io/badge/license-EUPL--1.2-blue"></a>
  <img alt="rust: nightly" src="https://img.shields.io/badge/rust-nightly-orange">
  <img alt="ngsi-ld v1.9.1" src="https://img.shields.io/badge/ngsi--ld-v1.9.1-8a2be2">
  <a href="../../actions/workflows/ci.yaml?query=branch%3Amain"><img alt="ci" src="../../actions/workflows/ci.yaml/badge.svg?branch=main&event=push"></a>
  <a href="../../actions/workflows/suite.yaml"><img alt="suite" src="../../actions/workflows/suite.yaml/badge.svg"></a>
  <a href="../../actions/workflows/advisories.yaml"><img alt="advisories" src="../../actions/workflows/advisories.yaml/badge.svg"></a>
</p>

---

<h3 align="center"><em>Andromeda would rather say no than be wrong.</em></h3>

<p align="center">
  It's an NGSI-LD context broker that checks every request against the semantics of the standard, not just its syntax,<br>
  and refuses what doesn't fit, with an error that names the field and the rule.<br>
  <strong>It sounds strict, and it is.</strong> Nothing wrong gets in, so nothing wrong comes out.
</p>

## Where it fits

Andromeda holds the current state of the things a system cares about and the history of how they got there. Applications read, query and subscribe to that state through one standard API. Anything that has a place, changes over time and is shared between parties fits. Cities come first: they are where NGSI-LD is used most, and Andromeda is shaped and tested around their data.

It runs in each of the architectures NGSI-LD describes:

- **Centralised.** One broker holds the data and answers every request.
- **Distributed.** Context sources register what they hold, and Andromeda forwards the matching part of each request and merges the answers. In a data space it takes the other side, as a source that keeps its data and shares it on its terms.
- **Federated.** The sources are themselves brokers, run by other teams or cities, so one query reaches all of them.

## Why Andromeda

### Your data stays correct

Many systems read from the same broker, so any mistake it accepts reaches all of them. Andromeda is built to accept none.

- Andromeda checks what a request means, not only its shape, and refuses anything the specification doesn't allow. It never guesses what you meant, rewrites your data or quietly drops part of it.
- Where the specification leaves a choice open, Andromeda picks one answer and uses it on every endpoint.
- Numbers keep their full precision, up to 131,072 digits before the decimal point and 16,383 after. They are stored exactly and never pass through floating point, so every digit you write comes back unchanged.
- Timestamps keep microsecond precision, the finest that ETSI GS CIM 009 allows.
- Each change and its history are written in the same transaction, so the history never falls behind or misses an update.
- Data from context sources goes through the same checks as a local write. If a source returns a bad entity, Andromeda leaves it out, reports it and still returns the rest of the answer.

### It keeps working when something fails

- If the database goes down, requests get a `503` with `Retry-After`, and a client can safely repeat any write.
- Background work pauses during an outage and picks up where it left off once the database is back, without losing anything.
- When a context source keeps failing, Andromeda leaves it alone for a cooldown period instead of letting it slow down every query.
- Cursor paging never skips or repeats an entity, even when the data changes between pages.
- The core `@context` is built in, so the broker works without internet access.

### Notifications get through

- Notifications go out over HTTP or MQTT (version 3.1.1 or 5, at any QoS level).
- In durable mode, each notification is queued in the same transaction as the write that caused it. If delivery fails, Andromeda retries and waits a little longer each time.
- When an endpoint keeps failing, Andromeda sends it one notification at a time until it answers, then delivers the backlog in the order it was queued.
- A slow endpoint doesn't hold anyone else up, and every other subscription keeps getting its notifications on time.
- A retried notification keeps its `id`, so a receiver can recognise a repeat and drop it.
- You can block notifications to loopback, private and reserved addresses. The check runs against the address the host name resolves to at the moment each notification is sent.

### It looks after its own database

- Built-in maintenance vacuums and analyzes tables and rebuilds bloated indexes. It runs only while the database is quiet and logs every decision it makes. With online repacking turned on, it also returns free space to the disk.
- A write that changes nothing leaves the database untouched.
- Attribute names and types are stored once, and rows point to them instead of repeating them.

### It is simple to run

- The [deployment planner](https://andromeda.skvarc.com/docs/deployment/setup/deployment-planner/) asks about your setup and gives you a ready configuration, a Compose file, reverse-proxy settings, PostgreSQL tuning and a checklist of steps. It can also check a configuration you already have, without anything leaving your browser.
- All it needs is a PostgreSQL database with PostGIS and TimescaleDB. There's no message queue, cache or separate history service to run alongside it.
- Prometheus metrics and the liveness and readiness probes are served on a separate port.
- It speaks HTTP/1.1, HTTP/2 and HTTP/3, and picks up rotated TLS certificates without a restart.
- Browser applications are covered too, with built-in CORS, response compression, security headers and CSRF protection.
- Each tenant gets its own isolated PostgreSQL database, created on its first write.
- PgBouncer is supported for deployments with many tenants that sit idle most of the time.

### More when you ask for it

Andromeda adds a few capabilities the standard doesn't define. None of them changes anything until a client asks for it. When one is switched off, its parameters and routes disappear and it is no longer advertised, so clients see a plain conformant broker. A single setting turns them all off.

### Built to be right

Andromeda is written in Rust, so memory corruption and data races are ruled out and there are no garbage collector pauses. Its types are designed so that an invalid value can't be constructed, and a case the code doesn't handle stops the build. More than 17,000 tests cover it. Many of them form a black-box suite that drives a running broker through its API, and that suite includes the ETSI NGSI-LD conformance tests, the examples from annex C of the specification, and distributed and interoperability scenarios.

### Built to be fast

Requests are decoded straight into typed values with few allocations. The database schema is shaped around the way NGSI-LD reads and writes data, and every query is planned against indexes built for it. In my testing so far it performs very well, but I'm not publishing figures yet. A speed claim only means something when anyone can repeat the measurement, and a reproducible benchmark suite is on the way. Until then, try it on your own data and workload. If something is slow, [get in touch](#community), because I treat a slow case as a bug.

## Try it

All you need is Docker with Compose, or Podman. The Compose file starts the broker and its database together. Download it, generate a database password, and start it:

```bash
curl -fsSLO https://raw.githubusercontent.com/mskvarc/andromeda/main/docker/compose/docker-compose.yaml
echo "POSTGRES_PASSWORD=$(openssl rand -hex 24)" > .env
docker compose up -d
```

The broker listens on `http://localhost:1056`. Andromeda is compatible with [Smart Data Models](https://smartdatamodels.org/). An entity written to a published model, with its published `@context`, works as it is. The example follows the `BikeHireDockingStation` model, and every request names its `@context` in a `Link` header:

```bash
CONTEXT='<https://smart-data-models.github.io/dataModel.Transportation/context.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json"'
```

Put three of Ljubljana's BicikeLJ bike-share stations on the map, with the bikes each one has at eight in the morning:

```bash
curl -i http://localhost:1056/ngsi-ld/v1/entityOperations/create \
  -H 'Content-Type: application/json' \
  -H "Link: $CONTEXT" \
  -d '[
      {
          "id": "urn:ngsi-ld:BikeHireDockingStation:presernov-trg",
          "type": "BikeHireDockingStation",
          "name": "Prešernov trg",
          "status": "empty",
          "totalSlotNumber": 20,
          "availableBikeNumber": {
              "value": 0,
              "observedAt": "2026-11-06T08:00:00Z"
          },
          "freeSlotNumber": {
              "value": 20,
              "observedAt": "2026-11-06T08:00:00Z"
          },
          "location": {
              "type": "Point",
              "coordinates": [
                  14.50673,
                  46.05137
              ]
          }
      },
      {
          "id": "urn:ngsi-ld:BikeHireDockingStation:kongresni-trg",
          "type": "BikeHireDockingStation",
          "name": "Kongresni trg",
          "status": "working",
          "totalSlotNumber": 20,
          "availableBikeNumber": {
              "value": 7,
              "observedAt": "2026-11-06T08:00:00Z"
          },
          "freeSlotNumber": {
              "value": 13,
              "observedAt": "2026-11-06T08:00:00Z"
          },
          "location": {
              "type": "Point",
              "coordinates": [
                  14.50445,
                  46.05042
              ]
          }
      },
      {
          "id": "urn:ngsi-ld:BikeHireDockingStation:trg-of",
          "type": "BikeHireDockingStation",
          "name": "Trg OF",
          "status": "working",
          "totalSlotNumber": 25,
          "availableBikeNumber": {
              "value": 12,
              "observedAt": "2026-11-06T08:00:00Z"
          },
          "freeSlotNumber": {
              "value": 13,
              "observedAt": "2026-11-06T08:00:00Z"
          },
          "location": {
              "type": "Point",
              "coordinates": [
                  14.51026,
                  46.05732
              ]
          }
      }
  ]'
```

Where can you get a bike within 400 m of Prešeren Square?

```bash
curl -G http://localhost:1056/ngsi-ld/v1/entities \
  -H "Link: $CONTEXT" \
  --data-urlencode 'type=BikeHireDockingStation' \
  --data-urlencode 'q=availableBikeNumber>0' \
  --data-urlencode 'georel=near;maxDistance==400' \
  --data-urlencode 'geometry=Point' \
  --data-urlencode 'coordinates=[14.50602,46.05137]' \
  --data-urlencode 'options=keyValues' \
  --data-urlencode 'pick=name,availableBikeNumber'
```

```json
[
    {
        "availableBikeNumber": 7,
        "name": "Kongresni trg"
    }
]
```

Prešernov trg is closer, but it is empty, and Trg OF has bikes but is 700 m away, by the railway station. At a quarter past eight, three riders bring bikes back to Prešernov trg:

```bash
curl -X PATCH http://localhost:1056/ngsi-ld/v1/entities/urn:ngsi-ld:BikeHireDockingStation:presernov-trg/attrs \
  -H 'Content-Type: application/json' \
  -H "Link: $CONTEXT" \
  -d '{
      "status": "working",
      "availableBikeNumber": {
          "value": 3,
          "observedAt": "2026-11-06T08:15:00Z"
      },
      "freeSlotNumber": {
          "value": 17,
          "observedAt": "2026-11-06T08:15:00Z"
      }
  }'
```

Every change is kept, so the station's morning is there to read back:

```bash
curl -G http://localhost:1056/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:BikeHireDockingStation:presernov-trg \
  -H "Link: $CONTEXT" \
  --data-urlencode 'attrs=availableBikeNumber' \
  --data-urlencode 'options=temporalValues'
```

```json
{
    "id": "urn:ngsi-ld:BikeHireDockingStation:presernov-trg",
    "type": "BikeHireDockingStation",
    "availableBikeNumber": {
        "type": "Property",
        "values": [
            [
                0,
                "2026-11-06T08:00:00Z"
            ],
            [
                3,
                "2026-11-06T08:15:00Z"
            ]
        ]
    }
}
```

Station locations are from [OpenStreetMap](https://www.openstreetmap.org/copyright).

Subscriptions, history, your own vocabulary and tenants are in the [documentation](https://andromeda.skvarc.com/docs/getting-started/quickstart/).

Ready to run it for real? The [deployment planner](https://andromeda.skvarc.com/docs/deployment/setup/deployment-planner/) turns a few questions about your setup into the files and steps to deploy it.

## Roadmap

The roadmap is the issue tracker, and it's public. Every accepted issue gets a [milestone](../../milestones). The next release's milestone holds what's planned for it, and Backlog holds what's accepted but not scheduled yet. Each issue also carries an `area:` label for the part of the broker it touches, and the [project board](https://github.com/users/mskvarc/projects/2) shows everything in one view.

Missing something you need? Open a [feature request](../../issues/new/choose) and say what you'd use it for. What people build with Andromeda shapes what comes next.

## Community

Andromeda is young, and the people who pick it up early will shape it the most. Whether you run it in a data space, test your own client against it, or are just curious about NGSI-LD, you're welcome here.

Come say hello in [discussions](../../discussions). Tell me what you're building, what got in your way or what you'd like to see next. Questions of any size are welcome, and so is a simple "I tried it and it worked". If you're working on something similar, I'd love to compare notes.

Every client Andromeda meets makes it better. If the broker refuses something you think the standard allows, or answers differently from what the standard says, please [open an issue](../../issues). Conformance reports help more than anything else. Bugs and feature requests go there as well.

Is something slower than it should be? That deserves an [issue](../../issues) too, and it gets fixed fastest when it includes the request, roughly how much data you have and what you expected.

Every issue starts from a form, and [Contributing](CONTRIBUTING.md#opening-an-issue) explains which one fits what. If you'd rather not think about it, **Report a problem** and **Suggest an idea** are the shortcut. They only ask what you can know without reading the code, and I'll sort out the rest when I triage it.

Not everything has to be an issue. Anything you'd like to raise can go to [discussions](../../discussions), or [email me](mailto:andromeda@skvarc.com) if you'd rather keep it private.

Changes are welcome too. A typo fix can go straight to a pull request, and anything bigger starts as an issue. [Contributing](CONTRIBUTING.md) walks you through the workflow and the rules every change follows.

Found a security problem? Please don't open a public issue. The [Security Policy](SECURITY.md) explains how to report it privately.

Everyone here follows the [Code of Conduct](CODE_OF_CONDUCT.md), so it stays a place people want to come back to.

## Licence

Andromeda is licensed under the [European Union Public Licence v. 1.2](LICENSE.md) (EUPL-1.2). Third-party material it includes, and the licences that cover it, are listed in the [Notice](NOTICE.md).

---

<p align="center"><sub>Open source · Open standards · Open data 🇪🇺</sub></p>
