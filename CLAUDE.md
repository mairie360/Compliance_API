# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`Compliance_API` (crate `compliance_api`, image `ghcr.io/mairie360/compliance-api`, dev port **3004**) is the
GDPR compliance service of a Mairie 360 instance (MAIR-498, epic MAIR-284). One runs in every mairie's
instance, next to the business APIs, and concentrates what touches erasure and the detection of personal data,
so that the business services do not carry it:

- **Rights**: the only service holding the Keycloak and Resend admin rights and the erasure rights on S3 and
  Redis. No API and no BFF carries them.
- **Continuous deterministic scan**: logs in the ingestion chain (masked before storage, a log store can hardly
  erase a line afterwards); the database in regular passes guided by the personal data inventory
  (Devops/Database `gdpr/inventory.yaml`: rows past their retention, erased or long-archived accounts that keep
  usable data; the free text of messages is out of scope); S3, Keycloak, Resend; Redis keys with a long TTL only
  (the others expire by themselves).
- **Erasure**: any personal data that should no longer exist (leak in a log, data of an erased user, retention
  exceeded) is erased. When a user is erased the service propagates it: Keycloak account, Resend contact, S3
  objects, Redis keys and the user's backup encryption key (MAIR-500). Every step is replayable and retried.
- **Compliance journal**, without the data itself: which kind of data, where (service, storage, logger,
  column), when, and the log line with the value masked. It is also the mairie's proof of erasure (GDPR art. 28
  and 30).
- **Deterministic only**: no data leaves the instance.

Not now (future notes, do not implement): a local AI agent with a Mistral model hosted in the instance to
interpret the findings, and an anonymous export of the journal (line templates, kinds, places, dates) to a journal
shared by every mairie.

Generated from `API_template` (keep the shared skeleton in sync with it and the sibling APIs, see
`../CLAUDE.md`). The first PR only adapted the template (name, port 3004, compose stacks, CI, Renovate);
the scan, erasure and journal come next. The service ships no business endpoint yet (`v1` is an empty scope).

## Commands

Same aliases as the siblings (`.cargo/config.toml`): `cargo lint_check`, `cargo lint_fix`,
`cargo check_code` (clippy `-D warnings`), `cargo test` (Docker + GHCR pull access, testcontainers
via `mairie360_api_lib::test_setup::queries_setup::get_shared_db`), `cargo cov_test` (60 % line
gate, only `main.rs` and `lib.rs` excluded: handlers and validation count, test them), `cargo open_api`
(OpenAPI JSON on stdout).

Access denials are tested, not assumed (MAIR-419): `tests/auth_gate_test.rs` checks the JWT gate in front of
`/api` (no token, forged, expired, malformed, unknown or archived account, percent-encoded path). The lib answers
`401` for a bad token and `404` for a well-signed token naming an unknown or archived account. Every new endpoint
adds its own negative tests next to the positive ones: non-admin on an admin route, another user reading /
modifying / deleting the resource, an id in the body that differs from the id in the URL.

End-to-end harnesses (what CI runs on `main` after the dev release; each spins up its own stack from a
standalone compose file, so env/image changes must be mirrored in all of them): `./integration_test.sh`
(`docker-compose-integration.yml`, newman replaying `tests/postman/collection.json` with
`tests/postman/environment.json`), `./security_test.sh` (ZAP), `./performance_test.sh` (k6). The
collection is a Postman v2.1 export; its pre-request script forges HS256 JWTs with the stack's
`JWT_SECRET` (`jwt_admin` for the seeded Admin, `jwt_agent` for user 2 from `init-test.sql`, plus a
wrong-secret and an expired token), so a new API only adds requests for its endpoints. `baseUrl` is
overridden with `--env-var` by the compose file; the committed default targets `localhost:3004`.

In those three stacks the API service is `image: ${IMAGE_REF}` (no `build:` block): CI sets `IMAGE_REF` to the
published `ghcr.io/mairie360/<name>:dev-<sha>` image, and the scripts build `compliance-api:local` from
`development.Dockerfile` when it is empty. That image is distroless (no shell, no curl), so readiness is a
`compliance-ready` sidecar polling `/ready` that dependent services wait on (`service_completed_successfully`). Every
stack sets `API_DOCS_ENABLED=true` (shared `x-common-env` anchor): without it the API serves neither Swagger UI nor
`/api-docs/openapi.json`, which newman, ZAP and k6 read.

The ZAP scan is authenticated and blocking: `security-scan` waits for the `seeder`, injects a static admin JWT
(`sub=1`, signed with `JWT_SECRET=b"secret"`, see the comment in `docker-compose-security.yml`) on every request
and fails on any alert not set to `IGNORE` / `OUTOFSCOPE` in `.zap/rules.tsv` (no `-I`; the file is the same in
every API). `-O http://<service>:<port>` is required, the spec's `servers` being unreachable from the ZAP
container. ZAP fuzzes every field from the spec examples, so an example that does not deserialize or a `500`
(value too long for its column, NUL byte, unmapped constraint violation) fails the job: fix the example or validate
the input, don't silence the alert. Do not reject `<` / `>` in free text to please ZAP (MAIR-426): a value echoed
in a JSON body served with `nosniff` is not an XSS, escaping is the front's job. If ZAP reports such a reflection,
set that rule to `IGNORE` in `.zap/rules.tsv` with the reason, in every API.

Both the ZAP and k6 stacks carry the OpenAPI coverage gate (MAIR-194) from mairie360/CICD `tests/`, available
as `cicd-repo/` (checked out by CI, cloned by the scripts at the pinned `cicd_version` otherwise, override with
`CICD_VERSION`; gitignored). ZAP runs with `--hook zap_hooks.py` and fails when an operation of the served spec was
never reached, or when an operation requiring `jwt` only got 401/403. The auth rule relies on the spec:
`endpoints/swagger.rs::SecurityAddon` declares the `jwt` bearer scheme (the name every API uses) at the top level
and marks every operation outside `/api/` public (`security: []`), mirroring `main.rs`.

`mairie360_api_lib` (2.0+) refuses to start with a missing, short (< 32 bytes) or well-known `JWT_SECRET`
(MAIR-428). The four compose stacks keep the public test value `b"secret"` (the static ZAP / k6 admin token and the
Postman script sign with it) and set `JWT_ALLOW_WEAK_SECRET: "true"` next to it: without it the API panics at
startup and every stack fails. A deployment never sets that variable and gets its own random secret.

`load-test.js` is built on `coverage.js`: one handler per operation (`"METHOD /path"`), k6 aborts at init
otherwise; the spec it reads is the one served by the image under test, saved into the `openapi-spec` volume by
`compliance-ready`. Same shape as the five APIs (MAIR-195): the spec is split by HTTP method into a `reads` scenario
(GET, ramp up to 20 VUs, against fixtures created in `setup()` and removed in `teardown()`) and a `writes` scenario
(every other method, 2 VUs, each handler creating what it needs through `fixture()` and deleting it afterwards, so
handlers are order-independent), one `p(95)` threshold per `op` tag (200 ms reads, 500 ms writes) and
`http_req_failed < 1%`. k6 waits for the `seeder`, which runs with `ON_ERROR_STOP`. **Adding an endpoint = adding
its handler in `load-test.js`** (`readHandlers` or `writeHandlers`), nothing to do for ZAP. Seed in `init-test.sql`
the rows the spec's path examples point at (ZAP builds its requests from them) and only what the API cannot create.

Every leaf handler is mounted as `#[get("/")]` (etc.) inside its segment scope, so its URL ends with `/`: its
`#[utoipa::path]` must say `path = "/"` when the parent `doc.rs` nests it without a trailing slash, otherwise the
spec documents a URL actix answers `404` to. `tests/routing_test.rs` checks every published operation against the
mounted routes and catches it.

## Layout

- `src/main.rs` builds `AppState` from `REDIS_URL` + `DB_*` (the Postgres URL goes through
  `database::pg_url::build_pg_url`, which percent-encodes user, password and database name), serves Swagger UI at
  `/swagger-ui/` and the spec at `/api-docs/openapi.json` (the ZAP scan target) only when `API_DOCS_ENABLED=true`
  (never in production: consumers use the published package), mounts the public probes and
  `endpoints::config` under `/api` wrapped in the lib's `JwtMiddleware`.
- `src/endpoints/health.rs`: `GET /health` is the liveness probe (always `200 OK`, no dependency checked, so a
  Postgres outage does not restart every pod); `GET /ready` is the readiness probe (`SELECT 1` through
  `database::ping` and a Redis read, 2 s timeout each, `503 not ready: <deps>` otherwise). Point Kubernetes'
  `livenessProbe` at the first and `readinessProbe` at the second. At startup `main.rs` waits for Postgres
  (`health::wait_for_postgres`, 10 tries about 30 s in all) and exits with an error when it never answers: the lib
  would otherwise start without a pool and answer `500` to everything (MAIR-423). Redis stays optional at startup
  (the cache degrades to Postgres), `/ready` still reports it.
- `src/endpoints/` mirrors the URL path: each node has `mod.rs` (`config()`), and leaves have
  `endpoint.rs` (handler + `trigger_*` + error enum implementing `ResponseError` and
  `From<ApiLibError>`), `view.rs` (DTOs, private fields + getters) and `doc.rs` (utoipa), nested
  up to `endpoints/swagger.rs::ApiDoc` (prefix `/api/v1`).
- `src/endpoints/validation.rs`: request views with text fields implement `Validate` (length matching the
  Postgres column, no control character; `<`, `>` and `&` are legitimate text) and handlers extract them with
  `ValidatedJson` / `ValidatedQuery` instead of `web::Json` / `web::Query`, which answer `400` naming the field.
  Map the lib's `DbError::ForeignKeyViolation` / `UniqueViolation` to `4xx`, never `500`: call
  `endpoints::db_error::classify_db_error("<resource>/<op>", &e)` and map each `DbFailure` (`NotFound`, `Conflict`,
  `InvalidReference`, `Internal`) to a variant of the handler's error enum. Never `.map_err(|_| ...)`: the
  helper logs the cause, `Internal` at `error` (MAIR-421).
- Logs go through `tracing` (`tracing_subscriber` set up first thing in `main.rs`, level from `RUST_LOG`,
  default `info`), which also carries actix's request log. No `println!` / `eprintln!` in new code. Responses carry
  `X-Content-Type-Options: nosniff` (`DefaultHeaders` in `main.rs`).
- Ids: path and body ids are `u64` in the API and `INT4` in Postgres. Convert with the lib's `id_to_sql` /
  `id_from_sql` (or `i32::try_from` answering `400`), never `as i32`: `2^32 + 1` would wrap to `1` and alias
  another row (MAIR-422). `lib.rs` and `main.rs` deny `clippy::cast_possible_truncation`, `cast_possible_wrap`
  and `cast_sign_loss`, so `cargo check_code` refuses the cast; do not silence them.
- Lists are bounded (MAIR-425): a list route takes `endpoints::pagination::PageParams` (`limit` 1-500, default
  100, `offset`) and runs `LIMIT` / `OFFSET` from `page()`, answering the page plus `total`; a feed that grows
  forever (messages, history) pages by cursor instead; a time-range query caps the width of the range. Rate
  limiting is not done per API: every call comes from a BFF, so a per-IP limit here would throttle all users at
  once. It belongs to the ingress / BFF layer.
- `src/database/<resource>/<op>/view.rs`: query views implementing `ApiRequestDto`, run through
  `state.get_smart_db()`. `fetch_one`/`fetch_all` SQL must return one JSON column
  (`SELECT to_jsonb(t) FROM (...) t`).
- A write spanning several queries, and an access check followed by the action it guards, runs in one
  transaction (MAIR-420): `let mut tx = state.get_smart_db().begin().await?;`, the queries on `tx`, then
  `tx.commit().await?`. An early `?` drops `tx` and rolls back what already ran; never compensate by hand with a
  `DELETE` whose error is ignored. A single CTE statement is fine too. `tests/transaction_test.rs` is the example.

## CI and Renovate

`.github/workflows/cicd.yml` calls `mairie360/CICD` `APIs_cicd.yml` on pull requests (lint, build, tests) and
on pushes to `main` (plus releases and the three stacks). Its `integration_tests`,
`integration_and_security` and `performance_isolated` jobs run the three `*_test.sh` scripts with
`IMAGE_REF` set to the `dev-<sha>` image published by `release-dev`; no Postman variable or secret is needed. `renovate.json` is the
standard API config: org preset, GitHub Actions pinned by digest, the `cicd_version` custom manager and the
`mairie360/CICD` group (both references in `cicd.yml` bumped in one PR).

`auto-approve.yml` approves Renovate PRs on the PR author (`github.event.pull_request.user.login`, not
`github.actor`), with `pull-requests: write` only and the action pinned by SHA. Both `Dockerfile`s pin their base
images by digest (`tests/dockerfile_test.rs` enforces it) and build with `--locked`; the production one builds
the dependencies in their own cached layer first (MAIR-427). Every advisory ignored in `.cargo/audit.toml` carries
a comment saying why it does not apply and when to drop it: no bare ignore, and a patched version is taken with
`cargo update -p <crate>` rather than ignored.

## Pull request reviewers

Every PR requests a review from the whole team, minus its author: `CarolinHugo`, `LAURETbenjamin`, `MathTek` and `Quentintnrl` (`gh pr create … --reviewer CarolinHugo,LAURETbenjamin,MathTek`). `.github/CODEOWNERS` makes GitHub request them automatically as well.
