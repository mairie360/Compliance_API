#!/usr/bin/env bash

# Runs the Postman collection committed under tests/postman/ with newman
# (docker-compose-integration.yml). The exit code of the `newman` service is the exit code of
# this script, so CI fails on the first failing request (`--bail`). No Postman account or API
# key is needed.
#
# The API under test is the image named by IMAGE_REF. CI sets it to the published
# ghcr.io/mairie360/template-api:dev-<sha> image (the one promoted to staging and prod); when it is # change api name
# empty (local usage) the image is built from development.Dockerfile as template-api:local.

COMPOSE_FILE="docker-compose-integration.yml"
SERVICE_NAME="newman"

if [ -z "${IMAGE_REF:-}" ]; then
    echo "==> [0/4] IMAGE_REF is empty: building template-api:local from development.Dockerfile..."
    IMAGE_REF="template-api:local" # change api name
    docker build -f development.Dockerfile -t "$IMAGE_REF" . || exit 1
fi
export IMAGE_REF
echo "==> API image under test: $IMAGE_REF"

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
