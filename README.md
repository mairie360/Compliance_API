# Compliance API

GDPR compliance service of a **Mairie 360** instance (MAIR-498, epic MAIR-284), in Rust (actix-web + utoipa +
`mairie360_api_lib`), generated from `API_template`. One runs in every mairie's instance.

What it will do (the first version only adapts the template):

- hold, alone, the Keycloak and Resend admin rights and the erasure rights on S3 and Redis;
- scan continuously and deterministically: logs before they are stored (masked), the database guided by the
  personal data inventory (retention exceeded, erased or long-archived accounts), S3, Keycloak, Resend, long-TTL
  Redis keys;
- erase what should no longer exist and propagate the erasure of a user (Keycloak account, Resend contact, S3
  objects, Redis keys, per-user backup key), each step replayable;
- keep a compliance journal without the data itself (kind, place, time, masked log line), proof of erasure for
  the mairie.

No personal data leaves the instance. See `CLAUDE.md` for the architecture and the commands.

## Run it

```bash
docker compose up --build --watch    # dev stack: Postgres + migrations, Redis, the API on port 3004
cargo lint_check && cargo check_code && cargo test
./integration_test.sh                # newman, ./security_test.sh (ZAP), ./performance_test.sh (k6)
```

The three `*_test.sh` stacks run the image named by `IMAGE_REF` (CI: the published `dev-<sha>` image), or
build `compliance-api:local` from `development.Dockerfile` when it is empty.
