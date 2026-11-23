#!/usr/bin/env bash

# Runs the Postman collection committed under tests/postman/ with newman
# (docker-compose-integration.yml). The exit code of the `newman` service is the exit code of
# this script, so CI fails on the first failing request (`--bail`). No Postman account or API
# key is needed.
#
# The API under test is the image named by IMAGE_REF. CI sets it to the published
# ghcr.io/mairie360/compliance-api:dev-<sha> image (the one promoted to staging and prod); when it is
# empty (local usage) the image is built from development.Dockerfile as compliance-api:local.

COMPOSE_FILE="docker-compose-integration.yml"
SERVICE_NAME="newman"

if [ -z "${IMAGE_REF:-}" ]; then
    echo "==> [0/4] IMAGE_REF is empty: building compliance-api:local from development.Dockerfile..."
    IMAGE_REF="compliance-api:local"
    docker build -f development.Dockerfile -t "$IMAGE_REF" . || exit 1
fi
export IMAGE_REF
echo "==> API image under test: $IMAGE_REF"

# Fresh Ed25519 key pair for this run: the API gets its public key, the runners tokens signed with
# it like Core signs them (MAIR-428, MAIR-680, test_secrets.sh).
# shellcheck source=test_secrets.sh
source "$(dirname "$0")/test_secrets.sh" || exit 1
# The collection's callers (the Postman sandbox cannot sign EdDSA): a service (role service, only
# services call this API, MAIR-498), the plain agent 2 (init-test.sql, must be refused), a service
# token claiming Core's kid but signed with another key, and an expired one.
JWT_SERVICE="$(sign_jwt core-api service 3600)" || exit 1
JWT_AGENT="$(sign_jwt 2 user 3600)" || exit 1
JWT_FORGED="$(sign_jwt core-api service 3600 "$(openssl genpkey -algorithm ed25519 2>/dev/null)")" || exit 1
JWT_EXPIRED="$(sign_jwt core-api service -3600)" || exit 1
export JWT_SERVICE JWT_AGENT JWT_FORGED JWT_EXPIRED

echo "==> [1/4] Starting the stack and the newman collection..."
docker compose -f "$COMPOSE_FILE" up -d

echo "==> [2/4] Waiting for the newman collection to finish..."
docker compose -f "$COMPOSE_FILE" wait "$SERVICE_NAME"
EXIT_CODE=$?

echo "==> [3/4] Report (logs)..."
docker compose -f "$COMPOSE_FILE" logs "$SERVICE_NAME"

echo "==> [4/4] Cleaning up the containers..."
docker compose -f "$COMPOSE_FILE" down

echo "----------------------------------------"
echo "Final exit code: $EXIT_CODE"
echo "----------------------------------------"

exit $EXIT_CODE
