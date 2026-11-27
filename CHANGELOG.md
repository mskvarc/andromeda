# Changelog

## [1.1.0](https://github.com/mskvarc/andromeda/compare/v1.0.0...v1.1.0) - 2026-11-27

### Highlights

**Queries and Subscriptions give the same answer.** Every operation that takes a query, from Query Entities and temporal queries to Delete by query, Entity Maps, Snapshots, Subscriptions and registration discovery, now reads `q`, `scopeQ`, `geoQ`, `csf` and the language filter through one engine, in the database and in memory alike. Dozens of places where 1.0.0 read the same query two ways now follow ETSI GS CIM 009 v1.9.1, among them trailing paths into structured values and GeoJSON members, URI spellings, year-zero instants, `near` across the antimeridian and the instances a temporal query selects through. A cursor walk is one walk however its query is spelled, and a paged query sent by POST now carries its page links. [One filter, one meaning](https://andromeda.skvarc.com/blog/one-filter-one-meaning/) tells how every filter got one meaning and which tests keep it.

**Distributed operation follows the specification much more closely.** A request forwarded to a Context Source carries the client's whole query, with its `datasetId`, its language filter and the join a `q` over Linked Entities needs, and reaches only the sources registered for what it asks. A query split across sources applies `attrs`, `pick` and `omit` to the merged Entity. A Subscription's copies at Context Sources are recorded before they are sent, placed again when a placement was cut off, narrowed to what each registration covers and kept in step as registrations change, and a notification relayed from a source carries only what its registration covers. A follow-up under an Entity Map is held to every input of the request that created the map, and a forwarding loop is recognised whatever comments its `Via` header holds.

**Snapshots capture what the specification describes.** A capture split across Context Sources applies `q`, `geoQ` and `scopeQ` to the merged Entities, a query that fails keeps nothing it staged, and the execution details name the Context Sources that failed and report each failure as the API would. Temporal captures stay within the federated query limits and refuse an answer a source cut short.

**Notification delivery rides out outages.** A notification whose endpoint can't be reached, or answers that it is overloaded or unavailable, is retried for about an hour by default, and one that meets any other server error for a little over a minute. A `Retry-After` the endpoint sends is honoured. Throttling always sends the newest change, and a Subscription held by a failing endpoint resumes with the earliest change it owes. Leases and retries run on the database's clock, a Subscription's MQTT server going down no longer holds the broker not ready, and a `receiverInfo` entry can no longer set a header the broker writes itself.

**Large tenants answer faster.** A `q` whose conjuncts an index can answer is now keyed by the rarest of them, so on a tenant of 100,000 Entities two measured queries went from 41 ms to 3.4 ms and from 219 ms to 2.5 ms. A periodic notification reads only the Entities it selects, 2 statements per tick instead of 401 in the case measured, and matching a change against 10,000 Subscriptions takes a third of the time it did. Geo-queries no longer wait for the spatial index, which is seeded beside a tenant's first requests and rebuilt partition by partition while writes carry on. A new tenant's database is copied from a migrated template in about 50 ms of server time, where migrating an empty one took about 300 ms.

**The API holds to the specification at its edges.** An Entity past its `expiresAt` is no longer served while it waits for the expiry sweep. A `@context` that isn't valid JSON-LD is refused rather than cached, and a reloaded or deleted `@context` takes effect at once. Data-integrity seals are checked when they are written and stored typed, and refusals describe what is wrong in the API's own terms.

**The image tells you what it contains.** It carries its own dependency tree and the licences of the crates it ships, and each release attests SPDX and CycloneDX SBOMs and an OpenVEX document to every platform image and attaches them to the GitHub release. Readiness now pings the database over a connection of its own, logs are plain text when standard output isn't a terminal, every background task runs under supervision, and two tenant names can no longer map to one database.

### Upgrade notes

The [upgrade guide](https://andromeda.skvarc.com/docs/operations/upgrades/) has the steps and names every setting mentioned here.

- **Back up every database, then upgrade the broker before TimescaleDB.** A bug in 1.0.0 makes the first request to each tenant fail with `500` after any TimescaleDB upgrade, whether through a newer PostgreSQL image or on a server of your own. Release 1.1.0 fixes it and runs on the image 1.0.0 shipped with, so move the broker first.
- **Health probes moved.** `/ngsi-ld/v1/health`, `/ngsi-ld/v1/ready` and `/ngsi-ld/v1/live` are gone. Probe `/readyz` and `/livez` on the management plane instead. The image's own health check needs no change.
- **Andromeda's own endpoints moved to `/ngsi-ld/andromeda/v1`.** This covers the version, tenant management and the receiver brokers notify each other at, so clients of tenant management and anything reading the version change their paths.
- **Federations see a burst of Subscription updates.** The first time each tenant opens, the broker moves every Subscription it placed at a Context Source to the new receiver path, updating or placing again each one in the background.
- **Extension tokens carry a revision.** Every extension is now experimental and advertises its token with a revision, such as `cursorPagination/0.1`. A 1.0.0 broker and a 1.1.0 broker don't recognise each other's extensions, so upgrade every broker of a federation together, and update any client that looks for a token. The broker logs a warning at startup for each extension that is on, and while HTTP/3 is on.
- **Some stored data is removed or rewritten.** Every Entity Map is removed, and a request naming one is answered as for an expired map. Cursors issued before the upgrade are refused, so a client starts its walk again. Top-level `ngsildproof` attributes, and seals no write would accept, are removed. A stored scope query holding the bare `/#` beside other Scopes is rewritten to the expression it meant.
- **Tenants that stored an `ngsildproof` take longer to open once.** The upgrade reads their whole current state and history, the columnstore and every Snapshot included, before they serve. Other tenants aren't affected.
- **Subscriptions may be notified differently.** They now select what Query Entities selects, so some are notified where they weren't and others no longer are. An entity Subscription's `csf` chooses the Context Sources it reaches and no longer filters what it notifies.
- **Prefixed names and IRIs in queries.** In `q`, `csf` and `orderBy`, a prefixed name or an IRI is written as it is or as a JSON string, as in `ex:temperature>20` or `"ex:temperature">20`. At the start of a hop's braces an unquoted colon ends the hop's type list, so `sensor{ex:humidity}` reads `ex` as an entity type, where 1.0.0 read a prefixed attribute, and a prefixed attribute there is written `sensor{"ex:humidity"}`. An IRI holding a dot is always written as a JSON string. The upgrade quotes such names in every stored Subscription `q` and `csf` and in the queries Snapshots were captured by, so each keeps the meaning 1.0.0 gave it.
- **Some requests are now refused.** A `@context` that isn't valid JSON-LD, the NGSI-LD Null as an Entity type, `location`, `observationSpace` or `operationSpace` as anything but a GeoProperty, and `id`, `type` or `scope` in `notification.attributes` are refused with `400`. So is a write whose attribute names, `valueType`, `objectType` or `vocab` the `@context` expands to no IRI, and Entities 1.0.0 stored with such a value still read back as written. The changes below list the rest.
- **A Snapshot that can't take a migration keeps its tenant from opening.** Every Snapshot now takes each migration together with the live data, and the error names the Snapshot that failed. Snapshots are no longer quarantined.
- **Some settings changed.** A notification whose endpoint can't be reached, or answers `429`, `502`, `503` or `504`, is retried for about an hour by default rather than five times. Any other server error gets the new `notifications.delivery.max_server_error_attempts`, 5 by default, so a configuration that raised `notifications.delivery.max_attempts` raises it too. With file logging on and no directory set, a Linux service outside a user session, a container among them, writes its log files to `/var/log/andromeda`. A retry delay above a day, a tenant database prefix longer than 44 bytes and a federation source alias outside the HTTP token characters now stop the broker from starting. A request body limit raised above 256 MiB needs the new storage quota for posted queries raised with it.
- **A template database appears on the PostgreSQL server.** The broker copies it for every new tenant and builds it again whenever it is missing, and `pg_dumpall` leaves it out.
- **PostgreSQL 18.7 is optional.** If you move to the image that ships it, read its [release notes](https://www.postgresql.org/docs/18/release-18-7.html), which ask you to reindex indexes that bugs it fixes may have corrupted. Andromeda's GIN indexes fall under two of them, and so do the TOAST indexes of history chunks, which automatic maintenance rebuilds with `REINDEX TABLE CONCURRENTLY`. A query that fails with `missing chunk number` is fixed by a `REINDEX` of the table it names. If you take incremental backups, take a new full backup after the move. The newer image also runs no TimescaleDB background workers, which the broker doesn't need.

### Changes

#### 🚀 Features

- attach the spdx sboms as release assets
- **extensions:** mark every extension experimental
- honour retry-after on durable http notification delivery
- list the image's crates and attest vex
- type ngsildproof seals and their data integrity proof ([#65](https://github.com/mskvarc/andromeda/issues/65))

#### 🐛 Bug Fixes

- **query:** read unquoted names outside a hop ([#1045](https://github.com/mskvarc/andromeda/issues/1045))
- **query:** quote names after a bare last type
- **query:** split orderBy outside quoted names ([#1034](https://github.com/mskvarc/andromeda/issues/1034), [#1035](https://github.com/mskvarc/andromeda/issues/1035), [#1037](https://github.com/mskvarc/andromeda/issues/1037))
- **notification:** probe the earliest held change ([#1040](https://github.com/mskvarc/andromeda/issues/1040))
- **entities:** hold written names to clause 4.6.2 ([#1036](https://github.com/mskvarc/andromeda/issues/1036), [#1038](https://github.com/mskvarc/andromeda/issues/1038), [#1039](https://github.com/mskvarc/andromeda/issues/1039))
- **notification:** order deliveries by change ([#1040](https://github.com/mskvarc/andromeda/issues/1040))
- **tenancy:** register chunk-skipping columns once ([#1041](https://github.com/mskvarc/andromeda/issues/1041))
- **query:** quote names outside the q grammar ([#1034](https://github.com/mskvarc/andromeda/issues/1034), [#1035](https://github.com/mskvarc/andromeda/issues/1035), [#1037](https://github.com/mskvarc/andromeda/issues/1037))
- **temporal:** narrow split instances before lastn ([#916](https://github.com/mskvarc/andromeda/issues/916))
- **health:** answer the health command over a local socket in every management mode
- **registration:** rewrite only the string instants of stored time intervals ([#953](https://github.com/mskvarc/andromeda/issues/953))
- **registration:** return discovered registrations as stored under lang ([#1027](https://github.com/mskvarc/andromeda/issues/1027))
- **registration:** refuse lang on registration retrieval ([#1026](https://github.com/mskvarc/andromeda/issues/1026))
- **snapshot:** rewrite the bare /# beside other scopes in stored snapshot queries ([#872](https://github.com/mskvarc/andromeda/issues/872))
- **registration:** stop projecting registrations by pick and omit ([#1025](https://github.com/mskvarc/andromeda/issues/1025))
- **registration:** refuse attrs beside pick or omit in discovery ([#873](https://github.com/mskvarc/andromeda/issues/873))
- **query:** read a bare pattern to the end of its query term ([#1024](https://github.com/mskvarc/andromeda/issues/1024))
- **query:** accept the bare /# only as the whole scope query ([#872](https://github.com/mskvarc/andromeda/issues/872))
- drop a throttled delivery only for a newer one
- retry server errors briefly and defer a withheld retry
- ship the licences of the broker's crates in the image
- ship the notice in the broker image
- attest an sbom of each platform image
- retry failing notifications for about an hour
- refuse id, type and scope in notification.attributes ([#844](https://github.com/mskvarc/andromeda/issues/844))
- refuse a downloaded `@context` that is not valid json-ld before caching it ([#1012](https://github.com/mskvarc/andromeda/issues/1012))
- refuse a reload whose download is not valid json-ld ([#1008](https://github.com/mskvarc/andromeda/issues/1008))
- refuse a `@context` that is not valid json-ld ([#968](https://github.com/mskvarc/andromeda/issues/968))
- apply attrs, pick and omit to the merged entity of a split query ([#1015](https://github.com/mskvarc/andromeda/issues/1015))
- filter split snapshot temporal queries after merging ([#1014](https://github.com/mskvarc/andromeda/issues/1014))
- filter split Snapshot queries after merging ([#842](https://github.com/mskvarc/andromeda/issues/842))
- **snapshot:** prepare what a pass claimed ([#999](https://github.com/mskvarc/andromeda/issues/999))
- word refusal details in api terms ([#971](https://github.com/mskvarc/andromeda/issues/971))
- **registration:** refuse non-http endpoints ([#970](https://github.com/mskvarc/andromeda/issues/970))
- **subscription:** redirect copies to new receiver ([#1000](https://github.com/mskvarc/andromeda/issues/1000))
- measure modules whose name ends in build in the architecture gates ([#989](https://github.com/mskvarc/andromeda/issues/989))
- log plain text on stdout when it is not a terminal ([#990](https://github.com/mskvarc/andromeda/issues/990))
- leave jemalloc background threads off on apple targets ([#991](https://github.com/mskvarc/andromeda/issues/991))
- spell a forwarded subscription filter in the context of its copy and keep unreadable stored members ([#1001](https://github.com/mskvarc/andromeda/issues/1001))
- forward a subscription q with the member spelling the client wrote ([#1001](https://github.com/mskvarc/andromeda/issues/1001))
- compact the geoproperty of a retrieved subscription ([#1001](https://github.com/mskvarc/andromeda/issues/1001))
- compact each type of a retrieved subscription type selection ([#1001](https://github.com/mskvarc/andromeda/issues/1001))
- refuse drift before extension work and keep long updates alive
- update tenant extensions behind a pooler and for non-owner roles
- **postgresql:** keep the server at 18.6 under timescaledb 2.32.0
- **postgresql:** keep the server at the base image's version
- **postgresql:** connect the health check as the superuser to the initial database ([#988](https://github.com/mskvarc/andromeda/issues/988))
- read near alike in a query and a subscription across the antimeridian and between antipodal corners ([#967](https://github.com/mskvarc/andromeda/issues/967))
- enter a q trailing path only through a property holding one value ([#965](https://github.com/mskvarc/andromeda/issues/965))
- hold a request under an entity map to every input of the one that created it ([#856](https://github.com/mskvarc/andromeda/issues/856))
- forward the client's lang with a query and a retrieval ([#911](https://github.com/mskvarc/andromeda/issues/911))
- place a periodic subscription copy without watched attributes and relay only what the registration covers ([#963](https://github.com/mskvarc/andromeda/issues/963))
- forward the join a q over linked entity attributes is read under ([#962](https://github.com/mskvarc/andromeda/issues/962))
- link no map of a registration read once per block under an entity map ([#961](https://github.com/mskvarc/andromeda/issues/961))
- refuse a posted lang that is no language filter without calling it a query parameter ([#956](https://github.com/mskvarc/andromeda/issues/956))
- delete the entities and watched attributes a registration stops narrowing in a subscription copy ([#959](https://github.com/mskvarc/andromeda/issues/959))
- forward the linked entity retrieval with an entity map creation ([#958](https://github.com/mskvarc/andromeda/issues/958))
- forward an entity map creation with only what each registration covers ([#958](https://github.com/mskvarc/andromeda/issues/958))
- forward every client filter through the batch query binding and read lang as the whole filter ([#957](https://github.com/mskvarc/andromeda/issues/957), [#956](https://github.com/mskvarc/andromeda/issues/956), [#960](https://github.com/mskvarc/andromeda/issues/960))
- match a subscription's dataset ids against context source registrations ([#955](https://github.com/mskvarc/andromeda/issues/955))
- ask a context source only for the datasets its registration lists ([#954](https://github.com/mskvarc/andromeda/issues/954))
- read the bbox and the foreign members of a geoproperty in a q trailing path ([#951](https://github.com/mskvarc/andromeda/issues/951))
- forward a request with a dataset id only to the sources registered ([#950](https://github.com/mskvarc/andromeda/issues/950))
- compare a registration time interval's instants as date times in a csf ([#953](https://github.com/mskvarc/andromeda/issues/953))
- read a declared time value wherever it sits in a registration property ([#948](https://github.com/mskvarc/andromeda/issues/948))
- choose the context sources of an entity subscription by its csf ([#949](https://github.com/mskvarc/andromeda/issues/949), [#843](https://github.com/mskvarc/andromeda/issues/843))
- read the iso 8601 year 0000 ([#946](https://github.com/mskvarc/andromeda/issues/946))
- read a declared time value of a registration property correctly ([#945](https://github.com/mskvarc/andromeda/issues/945))
- read a compound member as a property's value ([#944](https://github.com/mskvarc/andromeda/issues/944), [#947](https://github.com/mskvarc/andromeda/issues/947))
- keep a csf as written and read a registration instant only in the datetime form ([#936](https://github.com/mskvarc/andromeda/issues/936))
- give a deletion of any family no target and read a csf instance member by its core term ([#935](https://github.com/mskvarc/andromeda/issues/935), [#941](https://github.com/mskvarc/andromeda/issues/941), [#943](https://github.com/mskvarc/andromeda/issues/943), [#936](https://github.com/mskvarc/andromeda/issues/936))
- give an instance holding no value no target and bar a node's own members as csf sub-properties ([#942](https://github.com/mskvarc/andromeda/issues/942), [#935](https://github.com/mskvarc/andromeda/issues/935), [#936](https://github.com/mskvarc/andromeda/issues/936), [#941](https://github.com/mskvarc/andromeda/issues/941), [#943](https://github.com/mskvarc/andromeda/issues/943))
- compare coordinates as the store writes them and read registration nodes alike ([#935](https://github.com/mskvarc/andromeda/issues/935), [#936](https://github.com/mskvarc/andromeda/issues/936), [#941](https://github.com/mskvarc/andromeda/issues/941), [#942](https://github.com/mskvarc/andromeda/issues/942))
- read context source properties, arrays and split settlements alike in every execution ([#937](https://github.com/mskvarc/andromeda/issues/937), [#938](https://github.com/mskvarc/andromeda/issues/938), [#939](https://github.com/mskvarc/andromeda/issues/939), [#940](https://github.com/mskvarc/andromeda/issues/940), [#935](https://github.com/mskvarc/andromeda/issues/935), [#936](https://github.com/mskvarc/andromeda/issues/936), [#941](https://github.com/mskvarc/andromeda/issues/941), [#942](https://github.com/mskvarc/andromeda/issues/942), [#943](https://github.com/mskvarc/andromeda/issues/943))
- keep a temporal instance only when q and the geo-query hold with its attribute reduced to it ([#916](https://github.com/mskvarc/andromeda/issues/916), [#881](https://github.com/mskvarc/andromeda/issues/881))
- read a temporal q and geo-query only over the instances inside the window ([#915](https://github.com/mskvarc/andromeda/issues/915))
- order language maps, declared numbers and booleans and non-finite numbers by their target kind ([#919](https://github.com/mskvarc/andromeda/issues/919), [#932](https://github.com/mskvarc/andromeda/issues/932))
- read a geoproperty's geojson members in q as the store writes them ([#924](https://github.com/mskvarc/andromeda/issues/924))
- compare a uri q operand in its normal form with a relationship object and as written elsewhere ([#918](https://github.com/mskvarc/andromeda/issues/918))
- select entities by attrs, pick and omit only through standing instances ([#931](https://github.com/mskvarc/andromeda/issues/931))
- read a context source property's sub-properties by the names its registrant wrote ([#913](https://github.com/mskvarc/andromeda/issues/913))
- match a q pattern against the string field only for a string value ([#912](https://github.com/mskvarc/andromeda/issues/912))
- read a csf existence term over its whole path and a sub-property beside its property's value ([#913](https://github.com/mskvarc/andromeda/issues/913))
- end no q path on an instance member followed by a trailing group, in sql too ([#930](https://github.com/mskvarc/andromeda/issues/930))
- read one type before a colon at the head of a q hop as a type guard ([#923](https://github.com/mskvarc/andromeda/issues/923))
- admit a comma before the fraction of a datetime or time q operand ([#922](https://github.com/mskvarc/andromeda/issues/922))
- read a relationship as unequal to every range in a subscription too ([#914](https://github.com/mskvarc/andromeda/issues/914))
- read a q segment naming a value member as a sub-attribute in a subscription too ([#929](https://github.com/mskvarc/andromeda/issues/929))
- drop entity csf filtering, refuse an empty selector id, run the q corpus on the host ([#926](https://github.com/mskvarc/andromeda/issues/926), [#927](https://github.com/mskvarc/andromeda/issues/927), [#911](https://github.com/mskvarc/andromeda/issues/911), [#843](https://github.com/mskvarc/andromeda/issues/843))
- take an id array in a subscription entity selector ([#917](https://github.com/mskvarc/andromeda/issues/917))
- read every instance member a q path ends on alike in a query and a subscription ([#847](https://github.com/mskvarc/andromeda/issues/847))
- refuse the ngsild-snapshot header on every route outside the core, temporal and subscription apis ([#849](https://github.com/mskvarc/andromeda/issues/849))
- ping the database for readiness over a connection of its own ([#846](https://github.com/mskvarc/andromeda/issues/846))
- escape control characters when writing a q string back out ([#848](https://github.com/mskvarc/andromeda/issues/848))
- name a tenant's database from a sha-256 digest of its name ([#851](https://github.com/mskvarc/andromeda/issues/851))
- refuse a receiverinfo key naming a header the broker writes itself ([#850](https://github.com/mskvarc/andromeda/issues/850))
- keep the broker ready while a subscription's mqtt server is down ([#907](https://github.com/mskvarc/andromeda/issues/907))
- narrow a subscription copy's entity selectors to the registration it is placed through ([#845](https://github.com/mskvarc/andromeda/issues/845))
- admit the link context of delete entity, delete attribute and purge entities before deleting ([#841](https://github.com/mskvarc/andromeda/issues/841))
- stop serving an entity past its expiresat before the expiry sweep deletes it ([#839](https://github.com/mskvarc/andromeda/issues/839))
- read a via hop whose comment holds commas, nesting or non-ascii octets, and drop an unreadable hop alone ([#840](https://github.com/mskvarc/andromeda/issues/840))
- refuse the ngsi-ld null as an entity type in a whole-payload write ([#838](https://github.com/mskvarc/andromeda/issues/838))
- ignore a deletedat a client writes on an attribute instance ([#836](https://github.com/mskvarc/andromeda/issues/836))
- refuse location, observationspace and operationspace as anything but a geo property ([#835](https://github.com/mskvarc/andromeda/issues/835))
- take top-level seals out of a branch before converting the seals left ([#65](https://github.com/mskvarc/andromeda/issues/65))
- take top-level seals out of a branch before converting the seals left ([#65](https://github.com/mskvarc/andromeda/issues/65))
- read the seal conversion's patterns alike whatever standard_conforming_strings is ([#65](https://github.com/mskvarc/andromeda/issues/65))
- drop the seal conversion functions once every branch is converted ([#65](https://github.com/mskvarc/andromeda/issues/65))
- convert stored seals as the broker would, rewriting only rows that hold one ([#65](https://github.com/mskvarc/andromeda/issues/65))
- keep a subscription copy claimed when the source's answer leaves the copy unknown ([#628](https://github.com/mskvarc/andromeda/issues/628))
- bring every snapshot of a tenant through each migration with its live schema ([#65](https://github.com/mskvarc/andromeda/issues/65))
- drop a seal whose sealed types hold a non-reference instead of failing the upgrade ([#65](https://github.com/mskvarc/andromeda/issues/65))
- refuse a registration alias that is no http token ([#830](https://github.com/mskvarc/andromeda/issues/830))
- refuse a source alias that is no http token ([#585](https://github.com/mskvarc/andromeda/issues/585))
- time queue leases and retries on the database clock ([#829](https://github.com/mskvarc/andromeda/issues/829))
- reuse compiled queries and expanded terms only while their `@context` documents are unchanged ([#812](https://github.com/mskvarc/andromeda/issues/812))
- state the three-ordinate position bound as the broker's own, not rfc 7946's ([#75](https://github.com/mskvarc/andromeda/issues/75))
- run the tenant janitor and the context hit flush under task supervision ([#45](https://github.com/mskvarc/andromeda/issues/45))
- stage the close when the server refuses a request head itself ([#124](https://github.com/mskvarc/andromeda/issues/124))
- point the pages of a posted query at the query the broker holds ([#631](https://github.com/mskvarc/andromeda/issues/631))
- time durable deliveries, retries and probes on the database clock ([#531](https://github.com/mskvarc/andromeda/issues/531))
- digest an entity map's type selection alike through either query binding ([#632](https://github.com/mskvarc/andromeda/issues/632))
- read count as its own parameter on the subscription and registration listings ([#630](https://github.com/mskvarc/andromeda/issues/630))
- settle a snapshot partial when one of its queries yields no result ([#644](https://github.com/mskvarc/andromeda/issues/644))
- capture snapshot temporal queries whose lastn pages backwards ([#643](https://github.com/mskvarc/andromeda/issues/643))
- record failed context sources beside a snapshot query's result ([#642](https://github.com/mskvarc/andromeda/issues/642))
- keep nothing of a snapshot query that fails after staging part of its result ([#641](https://github.com/mskvarc/andromeda/issues/641))
- refuse snapshot temporal captures a source cut short at no range ([#640](https://github.com/mskvarc/andromeda/issues/640))
- bound snapshot temporal captures by the federated query limits ([#639](https://github.com/mskvarc/andromeda/issues/639))
- classify failed snapshot queries as the api does ([#638](https://github.com/mskvarc/andromeda/issues/638))
- bound the merged snapshot capture by the federated entity limit ([#93](https://github.com/mskvarc/andromeda/issues/93))
- settle subscription copies by the source's answer and place abandoned ones again ([#628](https://github.com/mskvarc/andromeda/issues/628))
- record a subscription copy before sending it to the source ([#627](https://github.com/mskvarc/andromeda/issues/627))

#### ⚡ Performance

- **notification:** read held successor by index ([#1040](https://github.com/mskvarc/andromeda/issues/1040))
- **subscription:** route a type expression by its alternatives ([#898](https://github.com/mskvarc/andromeda/issues/898))
- build spatial index partitions beside the ones in use during a rebuild ([#997](https://github.com/mskvarc/andromeda/issues/997))
- hold a spatial index back-index record of one placement inline ([#387](https://github.com/mskvarc/andromeda/issues/387))
- pack spatial index entries into shared r-tree leaves ([#386](https://github.com/mskvarc/andromeda/issues/386))
- hold a point in the spatial index as its two coordinates ([#386](https://github.com/mskvarc/andromeda/issues/386))
- hold spatial index entity ids as their spelling ([#385](https://github.com/mskvarc/andromeda/issues/385))
- plan the bounded spatial catch-up read with its horizon ([#994](https://github.com/mskvarc/andromeda/issues/994), [#992](https://github.com/mskvarc/andromeda/issues/992))
- stream a spatial index rebuild's read into its partitions ([#388](https://github.com/mskvarc/andromeda/issues/388))
- run the bounded spatial catch-up read without parallel workers ([#994](https://github.com/mskvarc/andromeda/issues/994))
- seed a tenant's spatial index beside its first requests ([#993](https://github.com/mskvarc/andromeda/issues/993))
- take spatial index writes in off the geo query path ([#992](https://github.com/mskvarc/andromeda/issues/992))
- **postgresql:** turn timescaledb background workers off by default
- create tenant databases as copies of a migrated template
- probe a walked entity's values through its own rows by key ([#981](https://github.com/mskvarc/andromeda/issues/981))
- read a gated page's scopeq matches and census-bounded attrs carriers ([#983](https://github.com/mskvarc/andromeda/issues/983))
- test a walk's filters after its limit and count only named entities in the spatial index ([#983](https://github.com/mskvarc/andromeda/issues/983))
- read a geo-query's matches first on the keyed page and probe it per candidate ([#983](https://github.com/mskvarc/andromeda/issues/983))
- test a walk's filters on each entity it visits so a walk beside attrs keeps the identifier order ([#983](https://github.com/mskvarc/andromeda/issues/983))
- stop the in-memory spatial index at the candidates a narrowing can name ([#983](https://github.com/mskvarc/andromeda/issues/983))
- cut a geo-query's page by a short walk, a bounded spatial read and a full walk ([#983](https://github.com/mskvarc/andromeda/issues/983))
- join the search row of each walked entity so a gated walk keeps the identifier order ([#983](https://github.com/mskvarc/andromeda/issues/983))
- probe each walked entity on its own and read keyed candidates to one bound ([#981](https://github.com/mskvarc/andromeda/issues/981))
- read each keyed conjunct in turn and walk the selection in page order ([#981](https://github.com/mskvarc/andromeda/issues/981))
- key an own-value scalar equality and a number ordering in q ([#981](https://github.com/mskvarc/andromeda/issues/981))
- count a range from 20 000 candidates and read scope paths through their partial index ([#952](https://github.com/mskvarc/andromeda/issues/952), [#884](https://github.com/mskvarc/andromeda/issues/884))
- read a periodic notification's candidates through the query engine ([#933](https://github.com/mskvarc/andromeda/issues/933))
- render sql q from the normal form and key a conjunction by its census-bounded conjunct ([#911](https://github.com/mskvarc/andromeda/issues/911), [#884](https://github.com/mskvarc/andromeda/issues/884))
- prepare in-memory q filters by cost with a shared term memo and one geo relation reading ([#911](https://github.com/mskvarc/andromeda/issues/911))
- add the history tombstone check again without reading history ([#65](https://github.com/mskvarc/andromeda/issues/65))
- try an unretrievable `@context` once per batch ([#208](https://github.com/mskvarc/andromeda/issues/208))
- read only the registrations a batch write can reach ([#204](https://github.com/mskvarc/andromeda/issues/204))
- commit each change clock advance in the round trip that takes the clock ([#571](https://github.com/mskvarc/andromeda/issues/571))
- bound the pool connections writes queued on the change clock hold ([#629](https://github.com/mskvarc/andromeda/issues/629))

<details>
<summary>Internal changes (181)</summary>

#### 🔧 Refactor

- move the task runtime boundary checks into a gate
- mint v7 ids through `uuid`
- give the v1.1.0 migrations one shape for helpers, codes and calls
- name the stage conversions and document the name stages ([#1001](https://github.com/mskvarc/andromeda/issues/1001))
- store the subscription terms in the expanded stage ([#1001](https://github.com/mskvarc/andromeda/issues/1001))
- merge the entity selectors into one constraint across stages ([#1001](https://github.com/mskvarc/andromeda/issues/1001))
- hold the type selection as alternatives generic over its stage ([#1001](https://github.com/mskvarc/andromeda/issues/1001))
- hold the q trees generic over their name stage ([#1001](https://github.com/mskvarc/andromeda/issues/1001))
- add the expanded attribute and entity type iri names ([#1001](https://github.com/mskvarc/andromeda/issues/1001))
- read scopeq and projection lists through winnow productions ([#964](https://github.com/mskvarc/andromeda/issues/964))
- read q filters through scannerless winnow productions ([#964](https://github.com/mskvarc/andromeda/issues/964))
- read type selection through a shared winnow disjunctive form ([#964](https://github.com/mskvarc/andromeda/issues/964))
- align query-language docs and remaining names with the naming rules ([#964](https://github.com/mskvarc/andromeda/issues/964))
- name bindings and helpers after the query trees they hold ([#964](https://github.com/mskvarc/andromeda/issues/964))
- read every query language through fromstr and write it back through display ([#964](https://github.com/mskvarc/andromeda/issues/964))
- give each query language its own error under one aggregate ([#964](https://github.com/mskvarc/andromeda/issues/964))
- name the query-language files, trees and fields under one scheme ([#964](https://github.com/mskvarc/andromeda/issues/964))
- copy new tenant databases from a template kept at the broker's schema
- read a geo-query's matches alone and probe one entity for it ([#983](https://github.com/mskvarc/andromeda/issues/983))
- derive cursor digests and entity map fingerprints from one query identity ([#911](https://github.com/mskvarc/andromeda/issues/911))
- rank a stored scalar in an ordering by the target kind of its value kind ([#911](https://github.com/mskvarc/andromeda/issues/911))
- move entity ordering into the postgresql query crate's ordering module ([#911](https://github.com/mskvarc/andromeda/issues/911))
- render a csf over registration documents from the lowered reading ([#911](https://github.com/mskvarc/andromeda/issues/911))
- resolve every q name once and fold a filter naming an unknown attribute ([#911](https://github.com/mskvarc/andromeda/issues/911))
- decide sql q verdicts by target kinds and one complement condition ([#911](https://github.com/mskvarc/andromeda/issues/911))
- render every sql q filter over attribute rows from the lowered reading ([#911](https://github.com/mskvarc/andromeda/issues/911))
- put every spelling of a q filter in one normal form ([#911](https://github.com/mskvarc/andromeda/issues/911))
- run every in-memory q filter on its reading, compiled once with the expression ([#911](https://github.com/mskvarc/andromeda/issues/911))
- lower q filters to a reading whose verdicts derive from test and polarity ([#911](https://github.com/mskvarc/andromeda/issues/911))
- place every stored value by one table the writers and readers share ([#911](https://github.com/mskvarc/andromeda/issues/911))
- move the stored form out of the query crate into stored-values and stored-form ([#911](https://github.com/mskvarc/andromeda/issues/911))
- admit a q once at validation and compile only admitted filters ([#928](https://github.com/mskvarc/andromeda/issues/928), [#911](https://github.com/mskvarc/andromeda/issues/911))
- compile every q filter through one entry ([#925](https://github.com/mskvarc/andromeda/issues/925), [#911](https://github.com/mskvarc/andromeda/issues/911))
- select entities through one entity selector ([#911](https://github.com/mskvarc/andromeda/issues/911))
- admit an entity by its q, geo-query and scope-query in one place ([#911](https://github.com/mskvarc/andromeda/issues/911))
- move in-memory query evaluation and the value orders into crates of their own ([#911](https://github.com/mskvarc/andromeda/issues/911))
- remove the residual query path and the reader capability profiles ([#911](https://github.com/mskvarc/andromeda/issues/911))
- name a posted query's digest a digest throughout ([#631](https://github.com/mskvarc/andromeda/issues/631))
- store a posted query's digest as its bytes
- **http:** serve non-standard routes under /ngsi-ld/andromeda/v1
- run the mqtt connection health probe as a supervised scheduled task ([#518](https://github.com/mskvarc/andromeda/issues/518))
- start create-time snapshot preparation on the tenant's supervised slots ([#517](https://github.com/mskvarc/andromeda/issues/517))
- render inline sql text literals independently of standard_conforming_strings ([#593](https://github.com/mskvarc/andromeda/issues/593))

#### 📚 Documentation

- **seal:** say why a seal's created is served with the decimal point ([#65](https://github.com/mskvarc/andromeda/issues/65))
- **subscription:** say in the branch scope query rewrite what it stores ([#872](https://github.com/mskvarc/andromeda/issues/872))
- explain what a replayed side-effect job may do ([#900](https://github.com/mskvarc/andromeda/issues/900))
- describe the q parser and its type guard in the grammar header ([#964](https://github.com/mskvarc/andromeda/issues/964))
- state the trade the keyed candidate bounds make ([#934](https://github.com/mskvarc/andromeda/issues/934), [#884](https://github.com/mskvarc/andromeda/issues/884))
- cite the clauses snapshot failures and result limits rest on ([#638](https://github.com/mskvarc/andromeda/issues/638))

#### 📦 Dependencies

- move the postgresql image to postgresql 18.7 and timescaledb 2.32.1
- re-render patterns for regex-syntax
- bump the cargo-minor group with 2 updates
- bump shopify/toxiproxy
- bump distroless/cc-debian13
- update the postgresql image to timescaledb 2.32.0 and postgis 3.7.0
- bump urn-rs and ngsi-ld

#### 🏗️ Build

- update sccache to 0.19.0
- pin squawk 2.73.0
- report unmaintained transitive crates on every cargo-deny version
- generate the vex document from the advisory rulings
- hold advisory rulings to the patched release they rely on
- attest the sboms of each postgresql platform image
- attest a cyclonedx sbom of each platform image
- state every advisory ruling to every scanner
- publish the vex document as a release asset
- link internal changes by compare range
- pin rust 1.100.0 images to the toolchain ([#998](https://github.com/mskvarc/andromeda/issues/998))
- switch the workspace to stable rust 1.100.0 ([#998](https://github.com/mskvarc/andromeda/issues/998))
- allow unmaintained chrono for time being
- install the pinned toolchain in the chef stage
- format the suite manifest and drop unused dev-dependencies ([#911](https://github.com/mskvarc/andromeda/issues/911))
- rework the changelog and release notes
- make test-only dependency optional
- check in the composed lexicon grammars
- check that images install the pinned toolchain ([#592](https://github.com/mskvarc/andromeda/issues/592))
- move to nightly-2026-09-25 on the old trait solver ([#592](https://github.com/mskvarc/andromeda/issues/592))
- drop git-derived stuff from version endpoint ([#611](https://github.com/mskvarc/andromeda/issues/611), [#618](https://github.com/mskvarc/andromeda/issues/618))

#### 👷 CI

- update cargo-nextest to 0.9.152
- update cargo-machete to 0.10.1
- update cargo-deny to 0.22.0
- label the rust-toolchain group entries build
- drop nightly-only checksum freshness ([#998](https://github.com/mskvarc/andromeda/issues/998))
- give every target the image context leaves out an empty file ([#911](https://github.com/mskvarc/andromeda/issues/911))
- optimize ci running time
- clear actionlint findings in ci and release ([#615](https://github.com/mskvarc/andromeda/issues/615), [#617](https://github.com/mskvarc/andromeda/issues/617))
- fix the review findings in keys, cleanup, pruning and suite runs ([#612](https://github.com/mskvarc/andromeda/issues/612), [#614](https://github.com/mskvarc/andromeda/issues/614), [#615](https://github.com/mskvarc/andromeda/issues/615), [#616](https://github.com/mskvarc/andromeda/issues/616))
- move off node 20 and the deprecated sbom attestation action ([#613](https://github.com/mskvarc/andromeda/issues/613))
- read image references from tags when pruning ([#612](https://github.com/mskvarc/andromeda/issues/612))
- key the images on what they compile and how they're built ([#612](https://github.com/mskvarc/andromeda/issues/612), [#614](https://github.com/mskvarc/andromeda/issues/614))
- run every suite tree on the commit's keyed images ([#612](https://github.com/mskvarc/andromeda/issues/612), [#614](https://github.com/mskvarc/andromeda/issues/614), [#615](https://github.com/mskvarc/andromeda/issues/615))
- keep compiled crates out of the image layer caches ([#613](https://github.com/mskvarc/andromeda/issues/613), [#614](https://github.com/mskvarc/andromeda/issues/614))
- record the build's commit only when its job passed ([#616](https://github.com/mskvarc/andromeda/issues/616))
- keep checksum freshness on with newer cargo ([#616](https://github.com/mskvarc/andromeda/issues/616))
- start main's build cache over on a dependency change ([#616](https://github.com/mskvarc/andromeda/issues/616))
- keep input images commit tags still point at ([#612](https://github.com/mskvarc/andromeda/issues/612), [#614](https://github.com/mskvarc/andromeda/issues/614), [#617](https://github.com/mskvarc/andromeda/issues/617))
- key the sql verdict on the seal ledger it verifies against ([#615](https://github.com/mskvarc/andromeda/issues/615))
- build the amd64 broker image per commit, reuse it on release ([#617](https://github.com/mskvarc/andromeda/issues/617))
- reuse unchanged workspace crates in clippy, unit and sql ([#616](https://github.com/mskvarc/andromeda/issues/616))
- skip a check whose inputs already passed on main ([#615](https://github.com/mskvarc/andromeda/issues/615))
- compile the test suite once per commit into the runner image ([#614](https://github.com/mskvarc/andromeda/issues/614))
- let sccache be kept in gha cache ([#613](https://github.com/mskvarc/andromeda/issues/613))
- build the broker image only when its inputs change ([#612](https://github.com/mskvarc/andromeda/issues/612))
- run the suite workflow on the images ci pushed for the commit ([#620](https://github.com/mskvarc/andromeda/issues/620))

#### 🧪 Tests

- **query:** pin unquoted member and sub-attribute names ([#1045](https://github.com/mskvarc/andromeda/issues/1045))
- **query:** pin unquoted name spelling ([#1045](https://github.com/mskvarc/andromeda/issues/1045))
- **query:** select through typed hop chains ([#1033](https://github.com/mskvarc/andromeda/issues/1033))
- **query:** pin csf name spelling ([#1034](https://github.com/mskvarc/andromeda/issues/1034))
- **upgrade:** read a release objectType back ([#1038](https://github.com/mskvarc/andromeda/issues/1038))
- **entities:** refuse bad types in every write ([#1038](https://github.com/mskvarc/andromeda/issues/1038))
- **notification:** settle two failures at once ([#1040](https://github.com/mskvarc/andromeda/issues/1040))
- **temporal:** open a registered tenant unchanged ([#1041](https://github.com/mskvarc/andromeda/issues/1041))
- **notification:** pin the probe of failed groups ([#1040](https://github.com/mskvarc/andromeda/issues/1040))
- **subscriptions:** quote a copy IRI q name ([#1034](https://github.com/mskvarc/andromeda/issues/1034))
- **query:** answer q differing in name ends apart ([#1037](https://github.com/mskvarc/andromeda/issues/1037))
- **query:** list a semicolon IRI q as one name ([#1035](https://github.com/mskvarc/andromeda/issues/1035))
- **entities:** pin objectType and vocab checks ([#1038](https://github.com/mskvarc/andromeda/issues/1038), [#1039](https://github.com/mskvarc/andromeda/issues/1039))
- **entities:** pin name refusals in every write ([#1036](https://github.com/mskvarc/andromeda/issues/1036))
- **entities:** type the name refusal bodies ([#1036](https://github.com/mskvarc/andromeda/issues/1036), [#1038](https://github.com/mskvarc/andromeda/issues/1038), [#1039](https://github.com/mskvarc/andromeda/issues/1039))
- **suite:** type the quoted q name requests ([#1034](https://github.com/mskvarc/andromeda/issues/1034), [#1035](https://github.com/mskvarc/andromeda/issues/1035))
- **query:** pin the cache keys of semicolon IRIs ([#1037](https://github.com/mskvarc/andromeda/issues/1037))
- **upgrade:** give each upgrade test its own hub ([#1034](https://github.com/mskvarc/andromeda/issues/1034))
- **query:** read back a semicolon IRI as one name ([#1035](https://github.com/mskvarc/andromeda/issues/1035))
- **query:** pin type lists in q hops ([#1033](https://github.com/mskvarc/andromeda/issues/1033))
- wait for the delivery state the broker records after a notification
- run the single-broker lang and projection discovery tests in the ngsi_ld tree
- pin that a scope change raises no attribute trigger
- give the scope hierarchy subscription test an attribute to trigger on ([#872](https://github.com/mskvarc/andromeda/issues/872))
- pin query-string plus and unencoded > ([#863](https://github.com/mskvarc/andromeda/issues/863), [#972](https://github.com/mskvarc/andromeda/issues/972))
- move the id pattern query test to 9018 ([#870](https://github.com/mskvarc/andromeda/issues/870))
- pin id pattern matching anywhere in the id ([#870](https://github.com/mskvarc/andromeda/issues/870))
- draw anchored filters, hops and geoqueries for filter parity ([#596](https://github.com/mskvarc/andromeda/issues/596))
- draw every attribute shape and geoqueries for filter parity ([#596](https://github.com/mskvarc/andromeda/issues/596))
- **postgresql:** move the settings check's user file off /run/user
- parse raw answers and walk trees with libs
- read only the status of registration discoveries that list other tests' registrations
- compare core contexts with their recorded digests only
- hold the etsi mock origin and wait for forwarded requests by stub rule
- expect clippy's grouped-import absolute paths in the context fault tests
- pin the system times of a sub-attribute added to an existing attribute ([#279](https://github.com/mskvarc/andromeda/issues/279))
- pin discovery by a whole entity type selection ([#902](https://github.com/mskvarc/andromeda/issues/902))
- release the executor gate before reporting ([#998](https://github.com/mskvarc/andromeda/issues/998))
- expand the filter compilation bench under the default vocabulary ([#1001](https://github.com/mskvarc/andromeda/issues/1001))
- expect a forwarded context source filter in the terms of its copy's context ([#1001](https://github.com/mskvarc/andromeda/issues/1001))
- send and read real unicode escapes in the escape tests ([#964](https://github.com/mskvarc/andromeda/issues/964))
- pin the path-depth refusal one segment past each path ceiling ([#964](https://github.com/mskvarc/andromeda/issues/964))
- pin what every query-language parse entry point makes of its inputs ([#964](https://github.com/mskvarc/andromeda/issues/964))
- **postgresql:** fix the image checks' history inserts and volume cleanup
- let the pgbouncer scale servers use 4 gb
- pin that a segment naming the value member of any family addresses nothing ([#966](https://github.com/mskvarc/andromeda/issues/966))
- pin that csf filters no entity a periodic notification carries ([#843](https://github.com/mskvarc/andromeda/issues/843))
- ask a csf over registration time intervals beside a temporal query and await csource notifications by registration ([#953](https://github.com/mskvarc/andromeda/issues/953))
- update a subscription's ranked lang and delete it with the ngsi-ld null ([#960](https://github.com/mskvarc/andromeda/issues/960))
- order a compound member declared as an xsd:boolean ([#932](https://github.com/mskvarc/andromeda/issues/932))
- run the csf parity grid in a private tenant and wait on every endpoint at once ([#936](https://github.com/mskvarc/andromeda/issues/936))
- fail the q reading gate on any site that reads q outside the reading ([#911](https://github.com/mskvarc/andromeda/issues/911))
- pin the sql statement shapes and the evaluator's target kinds against the stored kinds ([#911](https://github.com/mskvarc/andromeda/issues/911))
- walk the default tenant's subscriptions to the last page ([#911](https://github.com/mskvarc/andromeda/issues/911))
- add the q parity nets, statement snapshots and the q_reading gate ([#911](https://github.com/mskvarc/andromeda/issues/911), [#922](https://github.com/mskvarc/andromeda/issues/922), [#923](https://github.com/mskvarc/andromeda/issues/923), [#924](https://github.com/mskvarc/andromeda/issues/924))
- pin the query language divergences and read temporal q inside its window ([#871](https://github.com/mskvarc/andromeda/issues/871), [#912](https://github.com/mskvarc/andromeda/issues/912), [#913](https://github.com/mskvarc/andromeda/issues/913), [#914](https://github.com/mskvarc/andromeda/issues/914), [#915](https://github.com/mskvarc/andromeda/issues/915), [#916](https://github.com/mskvarc/andromeda/issues/916), [#917](https://github.com/mskvarc/andromeda/issues/917), [#918](https://github.com/mskvarc/andromeda/issues/918), [#919](https://github.com/mskvarc/andromeda/issues/919), [#843](https://github.com/mskvarc/andromeda/issues/843), [#881](https://github.com/mskvarc/andromeda/issues/881))
- read a 503 readiness answer ([#846](https://github.com/mskvarc/andromeda/issues/846), [#849](https://github.com/mskvarc/andromeda/issues/849))
- run the tests that oversubscribe a tenant pool alone
- read the expiry sweep's deletion on deletedat ([#839](https://github.com/mskvarc/andromeda/issues/839))
- run n_sub_cre_6814 and n_inf_ret_7004 in the private-tenants group ([#845](https://github.com/mskvarc/andromeda/issues/845))
- hold compact code registries to the last sealed release
- freeze every compact code v1.0.0 sealed
- run the snapshot quota tests in the private-tenants group
- re-pin snapshot cases whose only source cannot be reached ([#642](https://github.com/mskvarc/andromeda/issues/642))
- prove snapshot quota eviction waits for the snapshot it ranks first ([#490](https://github.com/mskvarc/andromeda/issues/490))
- correct n_map_qen_8001 cleanup comment ([#621](https://github.com/mskvarc/andromeda/issues/621))
- read relative expiries after the slow request ([#626](https://github.com/mskvarc/andromeda/issues/626))
- bring up broker b tenant with tenant helper ([#625](https://github.com/mskvarc/andromeda/issues/625))
- give o_fed_err_7213 and o_fed_err_7216 a 3000 ms forward timeout ([#624](https://github.com/mskvarc/andromeda/issues/624))
- give the wake backlog and clone tests five minutes ([#623](https://github.com/mskvarc/andromeda/issues/623))
- run the distributed tests at a size bound alone ([#622](https://github.com/mskvarc/andromeda/issues/622))
- give n_map_qen_8001 forwards a 20 s timeout ([#621](https://github.com/mskvarc/andromeda/issues/621))
- probe the suite stack postgres over tcp ([#619](https://github.com/mskvarc/andromeda/issues/619))

#### 🧹 Chores

- update dependencies
- update dependencies
- update dependencies
- update dependencies
- ignore machete false positive

</details>

## [1.0.0](https://github.com/mskvarc/andromeda/releases/tag/v1.0.0) - 2026-11-09

Initial release.

<!-- generated by git-cliff -->
