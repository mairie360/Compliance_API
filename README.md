# Rust API Template

Template des APIs Rust de **Mairie 360** (actix-web + utoipa + `mairie360_api_lib`). Il reprend le
squelette commun à `Core_API`, `Project_API`, `Calendar_API`, `Message_API` et `ELearning_API`.

## Créer une nouvelle API

### 1. Renommer l'API et choisir son port

Remplacer `template` / `api_template` / `template-api` / `TEMPLATE`, le port `3000` et l'URL
`template.development.mairie360.fr` partout où figure un marqueur `change api name` ou
`change port` (sur la ligne même, ou juste au-dessus dans les `Dockerfile` qui n'acceptent pas
de commentaire en fin de ligne) :

```bash
grep -rn "change api name\|change port" --exclude-dir=target --exclude-dir=.git .
```

`Cargo.lock` contient aussi le nom du crate : il est régénéré au premier `cargo build`.

Fichiers concernés : `Cargo.toml`, `src/main.rs`, `examples/generate_openapi.rs`, `Dockerfile`, `development.Dockerfile`, `entrypoint.sh`, `nginx.conf`,
`tests/pg_url_test.rs`, `docker-compose.yml`, `docker-compose-security.yml`, `docker-compose-performance.yml`,
`docker-compose-integration.yml`, `tests/postman/environment.json`, `.github/workflows/cicd.yml`.

### 2. Enable CI

`.github/workflows/cicd.yml` already runs on pull requests and on pushes to `main`. Integration
tests need no Postman account: the `integration_tests` job runs `./integration_test.sh`, which replays
`tests/postman/collection.json` with newman inside `docker-compose-integration.yml`. The CI jobs run
the three `*_test.sh` stacks against the published `dev-<sha>` image (`IMAGE_REF`); locally the scripts
build `<name>:local` from `development.Dockerfile` when `IMAGE_REF` is empty. Grow that
collection with the API's endpoints (the template only checks `/health`, `/ready`, the OpenAPI document
and the JWT gate; its pre-request script already forges HS256 JWTs with the stack's `JWT_SECRET`).

The ZAP and k6 stacks enforce an OpenAPI coverage gate (mairie360/CICD `tests/`, fetched into
`cicd-repo/`): every operation of the spec must be reached authenticated by ZAP, and `load-test.js`
must declare one handler per operation (`"METHOD /path"`), otherwise k6 aborts at init. Each new
endpoint therefore needs its handler in `load-test.js`; keep `SecurityAddon` in
`src/endpoints/swagger.rs` so the spec says which routes require the JWT.

### 3. Remettre la configuration Renovate standard

⚠️ Le `renovate.json` du template fusionne **toutes** les mises à jour sans condition, même quand
la CI échoue. Ce comportement est réservé au template : dans une vraie API, remplacer tout le
fichier par la configuration commune aux autres APIs :

```json
{
  "extends": [
    "github>mairie360/renovate-config"
  ],
  "customManagers": [
    {
      "customType": "regex",
      "fileMatch": ["^\\.github/workflows/cicd\\.yml$"],
      "matchStrings": [
        "cicd_version:\\s*(?<currentValue>v\\d+\\.\\d+\\.\\d+)"
      ],
      "depNameTemplate": "mairie360/CICD",
      "datasourceTemplate": "github-tags"
    }
  ]
}
```

### 4. Ajouter les endpoints

Le template ne contient aucun endpoint métier. Suivre la structure des autres APIs :
`src/endpoints/v1/<ressource>/<op>/` (`mod.rs`, `endpoint.rs`, `view.rs`, `doc.rs`, branchés dans
`v1/mod.rs` et `v1/doc.rs`), `src/database/<ressource>/<op>/view.rs` et les tests associés dans
`tests/queries/`.

Every endpoint ships its access-denial tests as well as its happy path (`tests/auth_gate_test.rs`
shows the pattern): non-admin on an admin route, another user reading / modifying / deleting the
resource, an id in the body that differs from the id in the URL. `endpoints/` counts in the 60 %
coverage gate.

A write that needs several queries (create a resource then its members, check access then act)
goes through one transaction: `state.get_smart_db().begin()`, the queries on the transaction, then
`commit()` (see `tests/transaction_test.rs`). An error in between rolls everything back.

A list endpoint never returns a whole table: it takes `endpoints::pagination::PageParams`
(`limit` capped at 500, `offset`) and answers one page plus the total count.

### 5. Documenter

Remplir `API.md` et adapter `CLAUDE.md`.

## Probes and API docs

- `GET /health`: liveness, always `200 OK`. `GET /ready`: readiness, `503` while Postgres or Redis
  does not answer. Point the chart's `livenessProbe` and `readinessProbe` at them.
- The API refuses to start when Postgres does not answer within about 30 s: the pod restarts
  instead of staying up and answering `500`.
- Swagger UI (`/swagger-ui/`) and `/api-docs/openapi.json` are served only when
  `API_DOCS_ENABLED=true`. Every compose stack sets it; production leaves it unset.
- `JWT_SECRET` must be random, at least 32 bytes and distinct per instance: `mairie360_api_lib`
  refuses to start with a short or well-known one. The compose stacks use the public test value
  `b"secret"` with `JWT_ALLOW_WEAK_SECRET=true`; never set that variable in a deployment.

## Commandes

```bash
cargo lint_check      # fmt --check            (CI)
cargo check_code      # clippy -D warnings     (CI)
cargo test            # nécessite Docker + accès ghcr.io/mairie360
cargo cov_test        # llvm-cov, 60 % line gate (main.rs and lib.rs excluded)
cargo open_api > openapi.json && npx orval   # client TypeScript dans generated/
docker compose up --watch                    # stack de dev (Postgres, Liquibase, Redis, nginx)
./integration_test.sh # newman replays tests/postman/collection.json (CI `integration_tests` job)
./security_test.sh    # OWASP ZAP scan
./performance_test.sh # k6 load test
```

## Renovate

Sur ce repo uniquement, `renovate.json` fusionne automatiquement **toutes** les mises à jour
(majeures comprises, 0.x et Dockerfile de prod inclus) sans attendre ni exiger une CI verte.
Voir l'étape 3 pour la configuration à utiliser dans une vraie API.
