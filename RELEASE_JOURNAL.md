# Journal des releases — Compliance_API

<!-- release-journal-state {"dev": "2ec813befa1c57a6f1e6a1d4dc438a3e594b906b", "prod": "", "prod_tag": "", "staging": "", "updated": {"dev": "2026-11-01 12:59 UTC"}} -->
> Tenu à jour par la CI (`mairie360/CICD`, action `release/release-journal`) à chaque passage en dev, staging et prod. Ne pas modifier à la main : le fichier est réécrit au passage suivant.

| Environnement | Version | Commit | Mis à jour |
| --- | --- | --- | --- |
| prod | — | — | — |
| staging | — | — | — |
| dev | — | `2ec813b` | 2026-11-01 12:59 UTC |

## Ajouté sur main depuis la dernière release prod (aucune release prod)

#### Nouveautés
- Premier service de conformité RGPD de l'instance : crate `compliance_api`, image `ghcr.io/mairie360/compliance-api`, port 3004. (MAIR-498, [#2](https://github.com/mairie360/Compliance_API/pull/2))
- `POST /api/v1/scans` lance le scan de conformité de la base via `fn_compliance_scan()`. Il cherche les lignes dont la rétention est dépassée, les comptes archivés au-delà de la politique `users` et les données restées sur des comptes anonymisés. Chaque constat est écrit dans `compliance_journal` et renvoyé en `200` : nombres et emplacements, jamais de valeur. La route renvoie `500` « The scan could not run. » en cas d'échec. Le même scan tourne en tâche de fond toutes les `SCAN_INTERVAL_SECONDS` (6 h par défaut, `0` le désactive). (MAIR-498, [#5](https://github.com/mairie360/Compliance_API/pull/5))
- Le scan couvre aussi les clés Redis de `REDIS_SCAN_URL` (optionnel), regroupées par préfixe. Il journalise `redis_no_ttl` et `redis_long_ttl` (TTL au-delà de `REDIS_LONG_TTL_SECONDS`, 30 jours par défaut), en nombre de clés par préfixe, jamais une clé ni une valeur. (MAIR-498, [#7](https://github.com/mairie360/Compliance_API/pull/7))
- `POST /api/v1/erasures/{userId}` lance ou reprend l'effacement centralisé d'un utilisateur et répond `202` avec les étapes. Il renvoie `404` « No such account. » pour un compte inconnu et `500` « The erasure could not be recorded. » si l'effacement ne peut pas être enregistré.
  - Ordre des étapes : compte Keycloak et contact d'audience Resend d'abord, puis la base (`anonymize_user`), puis les objets S3 sous `users/{user_id}/`, les clés Redis par motif et la clé de sauvegarde.
  - Chaque étape est idempotente, enregistrée dans `erasure_steps` et journalisée sans donnée.
  - Une étape en échec reste `failed` avec une raison sans valeur personnelle. Elle est relancée en tâche de fond (`ERASURE_RETRY_SECONDS`, 300 s par défaut).
  - Un service non configuré sur l'instance est noté `done` « not configured on this instance » (`erasure_skipped`).
  (MAIR-498, [#6](https://github.com/mairie360/Compliance_API/pull/6))
- `GET /api/v1/erasures/{userId}` renvoie les étapes de l'effacement et leur statut en `200`, ou `404` « No erasure was requested for this account. » si aucun effacement n'a été demandé. (MAIR-498, [#6](https://github.com/mairie360/Compliance_API/pull/6))
- `GET /api/v1/masking-patterns` sert les motifs de masquage des logs (`email`, `phone_fr`, `jwt`, `bearer`, `password_hash`) : regex RE2 et leur remplacement. Le collecteur OpenTelemetry les applique avant stockage. Les adresses IP ne sont volontairement pas masquées. (MAIR-498, [#7](https://github.com/mairie360/Compliance_API/pull/7))
- Chiffrement des sauvegardes avec une clé par utilisateur (crypto-shredding). Le nouveau binaire `/app/compliance-backup` est fourni dans l'image et s'exécute sur une copie jetable de la base, jamais sur la base vivante.
  - `seal` place les données personnelles de chaque utilisateur dans `users/<id>.json`, chiffré en AES-256-GCM avec une clé de données enveloppée par la clé `mairie360-user-<id>` du Key Manager Scaleway. Dans la copie, ces données sont remplacées par des valeurs de substitution.
  - `unseal` restaure les utilisateurs dont la clé existe encore. Un utilisateur effacé est restauré anonymisé.
  - L'étape « clé de sauvegarde » de l'effacement détruit désormais cette clé. Configuration : `SCW_SECRET_KEY`, `SCW_DEFAULT_PROJECT_ID`, `SCW_REGION`.
  - Diff partiel : le format exact des appels Scaleway reste à confirmer au premier usage réel.
  (MAIR-500, [#9](https://github.com/mairie360/Compliance_API/pull/9))

#### Changements de comportement
- Les routes sous `/api` n'acceptent plus les JWT des agents (`JwtMiddleware` retiré). Elles exigent un jeton de service HS256 signé avec `JWT_SECRET` et portant `role: service`. Sinon elles répondent `401` (« missing service token », « invalid service token » ou « not a service token »). (MAIR-498, [#5](https://github.com/mairie360/Compliance_API/pull/5))

#### Corrections
- L'image d'exécution passe de `gcr.io/distroless/cc-debian12` à `cc-debian13`, avec un builder `rust:1.99-slim-trixie`. Avant, l'image embarquait un `libssl3` vulnérable (CVE-2026-84782) ; elle n'a plus de vulnérabilité HIGH/CRITICAL corrigeable. (MAIR-741, [#21](https://github.com/mairie360/Compliance_API/pull/21))

#### Technique
- Socle et outillage :
  - initialisation depuis `API_template` ;
  - CICD v4.3.1 ([#15](https://github.com/mairie360/Compliance_API/pull/15)) ;
  - `mairie360_api_lib` 4.2.0, avec tests et stacks compose connectés sous le rôle Postgres `compliance_api` ([#13](https://github.com/mairie360/Compliance_API/pull/13)) ;
  - test Dockerfile Debian 13.
  (MAIR-571, MAIR-556)
- Mises à jour de dépendances (`actix-http`, `zstd`, `rustls`, `tokio`, etc.) pour les alertes RustSec ; RUSTSEC-2026-0387 est ignorée avec justification. (MAIR-740, [#20](https://github.com/mairie360/Compliance_API/pull/20))

## Différence dev → staging (en dev, pas encore en staging)

#### Nouveautés
- Premier service de conformité RGPD de l'instance : crate `compliance_api`, image `ghcr.io/mairie360/compliance-api`, port 3004. (MAIR-498, [#2](https://github.com/mairie360/Compliance_API/pull/2))
- `POST /api/v1/scans` lance le scan de conformité de la base via `fn_compliance_scan()`. Il cherche les lignes dont la rétention est dépassée, les comptes archivés au-delà de la politique `users` et les données restées sur des comptes anonymisés. Chaque constat est écrit dans `compliance_journal` et renvoyé en `200` : nombres et emplacements, jamais de valeur. La route renvoie `500` « The scan could not run. » en cas d'échec. Le même scan tourne en tâche de fond toutes les `SCAN_INTERVAL_SECONDS` (6 h par défaut, `0` le désactive). (MAIR-498, [#5](https://github.com/mairie360/Compliance_API/pull/5))
- Le scan couvre aussi les clés Redis de `REDIS_SCAN_URL` (optionnel), regroupées par préfixe. Il journalise `redis_no_ttl` et `redis_long_ttl` (TTL au-delà de `REDIS_LONG_TTL_SECONDS`, 30 jours par défaut), en nombre de clés par préfixe, jamais une clé ni une valeur. (MAIR-498, [#7](https://github.com/mairie360/Compliance_API/pull/7))
- `POST /api/v1/erasures/{userId}` lance ou reprend l'effacement centralisé d'un utilisateur et répond `202` avec les étapes. Il renvoie `404` « No such account. » pour un compte inconnu et `500` « The erasure could not be recorded. » si l'effacement ne peut pas être enregistré.
  - Ordre des étapes : compte Keycloak et contact d'audience Resend d'abord, puis la base (`anonymize_user`), puis les objets S3 sous `users/{user_id}/`, les clés Redis par motif et la clé de sauvegarde.
  - Chaque étape est idempotente, enregistrée dans `erasure_steps` et journalisée sans donnée.
  - Une étape en échec reste `failed` avec une raison sans valeur personnelle. Elle est relancée en tâche de fond (`ERASURE_RETRY_SECONDS`, 300 s par défaut).
  - Un service non configuré sur l'instance est noté `done` « not configured on this instance » (`erasure_skipped`).
  (MAIR-498, [#6](https://github.com/mairie360/Compliance_API/pull/6))
- `GET /api/v1/erasures/{userId}` renvoie les étapes de l'effacement et leur statut en `200`, ou `404` « No erasure was requested for this account. » si aucun effacement n'a été demandé. (MAIR-498, [#6](https://github.com/mairie360/Compliance_API/pull/6))
- `GET /api/v1/masking-patterns` sert les motifs de masquage des logs (`email`, `phone_fr`, `jwt`, `bearer`, `password_hash`) : regex RE2 et leur remplacement. Le collecteur OpenTelemetry les applique avant stockage. Les adresses IP ne sont volontairement pas masquées. (MAIR-498, [#7](https://github.com/mairie360/Compliance_API/pull/7))
- Chiffrement des sauvegardes avec une clé par utilisateur (crypto-shredding). Le nouveau binaire `/app/compliance-backup` est fourni dans l'image et s'exécute sur une copie jetable de la base, jamais sur la base vivante.
  - `seal` place les données personnelles de chaque utilisateur dans `users/<id>.json`, chiffré en AES-256-GCM avec une clé de données enveloppée par la clé `mairie360-user-<id>` du Key Manager Scaleway. Dans la copie, ces données sont remplacées par des valeurs de substitution.
  - `unseal` restaure les utilisateurs dont la clé existe encore. Un utilisateur effacé est restauré anonymisé.
  - L'étape « clé de sauvegarde » de l'effacement détruit désormais cette clé. Configuration : `SCW_SECRET_KEY`, `SCW_DEFAULT_PROJECT_ID`, `SCW_REGION`.
  - Diff partiel : le format exact des appels Scaleway reste à confirmer au premier usage réel.
  (MAIR-500, [#9](https://github.com/mairie360/Compliance_API/pull/9))

#### Changements de comportement
- Les routes sous `/api` n'acceptent plus les JWT des agents (`JwtMiddleware` retiré). Elles exigent un jeton de service HS256 signé avec `JWT_SECRET` et portant `role: service`. Sinon elles répondent `401` (« missing service token », « invalid service token » ou « not a service token »). (MAIR-498, [#5](https://github.com/mairie360/Compliance_API/pull/5))

#### Corrections
- L'image d'exécution passe de `gcr.io/distroless/cc-debian12` à `cc-debian13`, avec un builder `rust:1.99-slim-trixie`. Avant, l'image embarquait un `libssl3` vulnérable (CVE-2026-84782) ; elle n'a plus de vulnérabilité HIGH/CRITICAL corrigeable. (MAIR-741, [#21](https://github.com/mairie360/Compliance_API/pull/21))

#### Technique
- Socle et outillage :
  - initialisation depuis `API_template` ;
  - CICD v4.3.1 ([#15](https://github.com/mairie360/Compliance_API/pull/15)) ;
  - `mairie360_api_lib` 4.2.0, avec tests et stacks compose connectés sous le rôle Postgres `compliance_api` ([#13](https://github.com/mairie360/Compliance_API/pull/13)) ;
  - test Dockerfile Debian 13.
  (MAIR-571, MAIR-556)
- Mises à jour de dépendances (`actix-http`, `zstd`, `rustls`, `tokio`, etc.) pour les alertes RustSec ; RUSTSEC-2026-0387 est ignorée avec justification. (MAIR-740, [#20](https://github.com/mairie360/Compliance_API/pull/20))

## Différence staging → prod (en staging, pas encore en prod)

_Rien._

## Historique des passages

<!-- release-journal-history:start -->
| Date | Environnement | Version | Commit | Run |
| --- | --- | --- | --- | --- |
| 2026-11-01 12:59 UTC | dev | — | `2ec813b` | [run](https://github.com/mairie360/Compliance_API/actions/runs/41308132640) |
<!-- release-journal-history:end -->
