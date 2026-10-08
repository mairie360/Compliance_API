#!/usr/bin/env bash

# Runs the OWASP ZAP scan (docker-compose-security.yml) against the API. The exit code of the
# `security-scan` service is the exit code of this script: any WARN/FAIL alert fails CI.
#
# The API under test is the image named by IMAGE_REF. CI sets it to the published
# ghcr.io/mairie360/template-api:dev-<sha> image (the one promoted to staging and prod); when it is # change api name
# empty (local usage) the image is built from development.Dockerfile as template-api:local.

COMPOSE_FILE="docker-compose-security.yml"
SERVICE_NAME="security-scan"

if [ -z "${IMAGE_REF:-}" ]; then
    echo "==> [0/4] IMAGE_REF is empty: building template-api:local from development.Dockerfile..."
    IMAGE_REF="template-api:local" # change api name
    docker build -f development.Dockerfile -t "$IMAGE_REF" . || exit 1
fi
export IMAGE_REF
echo "==> API image under test: $IMAGE_REF"

# Shared CI test files (OpenAPI coverage gate, MAIR-194: ZAP hook and k6 coverage module). CI
# checks mairie360/CICD out as cicd-repo/; locally it is cloned once at the cicd_version pinned in
# .github/workflows/cicd.yml (override with CICD_VERSION, e.g. a branch not released yet).
CICD_DIR="cicd-repo"
if [ ! -f "$CICD_DIR/tests/k6/coverage.js" ]; then
    CICD_VERSION="${CICD_VERSION:-$(sed -n 's/^[[:space:]]*cicd_version:[[:space:]]*\([^[:space:]#]*\).*/\1/p' .github/workflows/cicd.yml | head -n 1)}"
    echo "==> Fetching mairie360/CICD $CICD_VERSION into $CICD_DIR/..."
    rm -rf "$CICD_DIR"
    git clone --quiet --depth 1 --branch "$CICD_VERSION" https://github.com/mairie360/CICD "$CICD_DIR" || exit 1
fi

echo "==> [1/4] Starting the stack and the ZAP scan..."
docker compose -f "$COMPOSE_FILE" up -d

echo "==> [2/4] Waiting for the ZAP scan to finish..."
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
