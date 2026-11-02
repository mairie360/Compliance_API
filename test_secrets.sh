#!/usr/bin/env bash
# Sourced by integration_test.sh, security_test.sh, performance_test.sh and gdpr_marker_test.sh
# (MAIR-428, MAIR-680).
#
# Only Core_API signs session JWTs (MAIR-680): this API gets Core's public key (JWT_PUBLIC_KEY) and
# no JWT_SECRET, so it accepts EdDSA tokens signed with Core's key and refuses every HS256 token.
# Each run plays Core with a fresh Ed25519 key pair (unless JWT_PRIVATE_KEY and JWT_PUBLIC_KEY are
# both exported) and signs here the tokens the runners need, the way Core signs them: `alg` EdDSA,
# `kid` = RFC 7638 thumbprint of the public key, `iss` mairie360-core, `aud` mairie360. Only the
# public key and the tokens leave this script: no key or token valid anywhere is committed, and the
# API never sees the private key.
#
# Exports JWT_PUBLIC_KEY and ADMIN_JWT (user 1, the Admin seeded by liquibase, valid 2 hours) and
# defines sign_jwt and sign_jwt_file. Needs openssl 3.

if [ -z "${JWT_PRIVATE_KEY:-}" ] || [ -z "${JWT_PUBLIC_KEY:-}" ]; then
    JWT_PRIVATE_KEY="$(openssl genpkey -algorithm ed25519 2>/dev/null)" || return 1
    JWT_PUBLIC_KEY="$(printf '%s\n' "$JWT_PRIVATE_KEY" | openssl pkey -pubout)" || return 1
fi
# The private key stays in this shell: only Core would hold it.
export -n JWT_PRIVATE_KEY 2>/dev/null
export JWT_PUBLIC_KEY

# Base64url without padding, from stdin.
b64url() {
    openssl base64 -A | tr '+/' '-_' | tr -d '='
}

# `kid` of the session tokens: RFC 7638 thumbprint of the Ed25519 public key (the last 32 bytes of
# its DER), computed like mairie360_api_lib does.
JWT_KID="$(printf '{"crv":"Ed25519","kty":"OKP","x":"%s"}' \
    "$(printf '%s\n' "$JWT_PUBLIC_KEY" | openssl pkey -pubin -outform DER | tail -c 32 | b64url)" \
    | openssl dgst -sha256 -binary | b64url)" || return 1

# sign_jwt <sub> <role> <ttl-seconds> [private key PEM]: EdDSA JWT with the claims Core signs
# ({ sub, role, exp, iss, aud }, no `sid`), under Core's `kid`. The optional key signs a forgery: a
# token claiming Core's `kid` but signed with another key, which the API must refuse.
sign_jwt() {
    local key="${4:-$JWT_PRIVATE_KEY}" header payload input signature
    header="$(printf '{"alg":"EdDSA","typ":"JWT","kid":"%s"}' "$JWT_KID" | b64url)"
    payload="$(printf '{"sub":"%s","role":"%s","exp":%d,"iss":"mairie360-core","aud":"mairie360"}' \
        "$1" "$2" "$(($(date +%s) + $3))" | b64url)"
    # `pkeyutl -rawin` (Ed25519) reads its input from a file, not from a pipe.
    input="$(mktemp)"
    printf '%s.%s' "$header" "$payload" >"$input"
    signature="$(openssl pkeyutl -sign -rawin -in "$input" -inkey <(printf '%s\n' "$key") | b64url)"
    rm -f "$input"
    [ -n "$signature" ] || return 1
    printf '%s.%s.%s' "$header" "$payload" "$signature"
}

# sign_jwt_file <file> <role> <ttl-seconds> <sub>...: writes { "<sub>": "<EdDSA JWT>", ... } to
# <file>, for k6, whose WebCrypto cannot sign Ed25519.
sign_jwt_file() {
    local file="$1" role="$2" ttl="$3" sub separator='{'
    shift 3
    : >"$file" || return 1
    for sub in "$@"; do
        printf '%s"%s":"%s"' "$separator" "$sub" "$(sign_jwt "$sub" "$role" "$ttl")" >>"$file" || return 1
        separator=','
    done
    printf '}\n' >>"$file"
}

ADMIN_JWT="$(sign_jwt 1 admin 7200)" || return 1
export ADMIN_JWT
