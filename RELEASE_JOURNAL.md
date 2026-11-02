# Journal des releases — Compliance_API

<!-- release-journal-state {"dev": "e9b68a0ca8c428c85670f2a223c1b5c265cd1d89", "prod": "", "prod_tag": "", "staging": "", "updated": {"dev": "2026-11-02 23:05 UTC"}} -->
> Tenu à jour par la CI (`mairie360/CICD`, action `release/release-journal`) à chaque passage en dev, staging et prod. Ne pas modifier à la main : le fichier est réécrit au passage suivant.

| Environnement | Version | Commit | Mis à jour |
| --- | --- | --- | --- |
| prod | — | — | — |
| staging | — | — | — |
| dev | — | `e9b68a0` | 2026-11-02 23:05 UTC |

## Ajouté sur main depuis la dernière release prod (aucune release prod)

#### Nouveautés
- `POST /api/v1/scans` lance le scan de la base et, si `REDIS_SCAN_URL` est défini, celui des clés Redis sans TTL ou à TTL supérieur à `REDIS_LONG_TTL_SECONDS` (30 jours par défaut). Il renvoie les constats (nombre et emplacement, jamais une valeur). Réservé au jeton de service (HS256, `role: service`). Le scan tourne aussi toutes les `SCAN_INTERVAL_SECONDS` (6 h par défaut, `0` désactive). (MAIR-498, [#5](https://github.com/mairie360/Compliance_API/pull/5), [#7](https://github.com/mairie360/Compliance_API/pull/7))
- `POST /api/v1/erasures/{userId}` démarre ou reprend l'effacement d'un compte : 202 avec les étapes, 404 si le compte est inconnu, 500 si l'effacement ne peut pas être enregistré. Ordre des étapes : compte Keycloak et contact Resend, puis `anonymize_user` en base, puis objets S3 `users/{user_id}/`, clés Redis et clé de sauvegarde. Chaque étape est journalisée sans donnée et relancée en arrière-plan (`ERASURE_RETRY_SECONDS`, 300 s par défaut). (MAIR-498, [#6](https://github.com/mairie360/Compliance_API/pull/6))
- `GET /api/v1/erasures/{userId}` renvoie les étapes de l'effacement et leur statut ; 404 si aucun effacement n'a été demandé pour ce compte. (MAIR-498, [#6](https://github.com/mairie360/Compliance_API/pull/6))
- `GET /api/v1/masking-patterns` (jeton de service) renvoie les motifs de masquage des logs : e-mail, téléphone français, JWT, bearer, hash argon2. Les adresses IP ne sont pas masquées. (MAIR-498, [#7](https://github.com/mairie360/Compliance_API/pull/7))

#### Changement de comportement
- Les routes `/api` n'acceptent plus le JWT des agents : seul un jeton HS256 signé avec `JWT_SECRET` et portant `role: service` passe, un jeton d'agent reçoit 401. Le `JwtMiddleware` de la lib n'est plus monté. (MAIR-498, [#5](https://github.com/mairie360/Compliance_API/pull/5))
- `POST /api/v1/scans` exécute désormais aussi le scan Redis quand `REDIS_SCAN_URL` est défini, et ses constats Redis s'ajoutent à la réponse. (MAIR-498, [#7](https://github.com/mairie360/Compliance_API/pull/7))
- Étape « clé de sauvegarde » de l'effacement : elle était toujours marquée « not configured on this instance ». Avec `SCW_SECRET_KEY`, `SCW_DEFAULT_PROJECT_ID` et `SCW_REGION`, elle détruit désormais la clé de l'utilisateur dans Scaleway Key Manager. (MAIR-500, [#9](https://github.com/mairie360/Compliance_API/pull/9))
- Sauvegardes : le binaire `compliance-backup` (scellement et restauration) est ajouté à l'image. Les données personnelles des sauvegardes sont chiffrées par utilisateur (AES-256-GCM) ; une sauvegarde dont la clé a été détruite restaure cet utilisateur anonymisé. (MAIR-500, [#9](https://github.com/mairie360/Compliance_API/pull/9))

#### Technique
- Squelette initial du service (commit `69d3fcb`, sans PR) puis adaptation depuis `API_template` : crate `compliance_api`, port 3004, Renovate standard, compose et README. (MAIR-498, [#2](https://github.com/mairie360/Compliance_API/pull/2))
- Dépendances : `cargo audit` corrigé (`Cargo.lock`, ignore de RUSTSEC-2026-0387 documenté), puis `zip` passé en 9.2.2 via `utoipa-swagger-ui`. Ajout de `aes-gcm`, `sqlx` et `testcontainers` pour le scellement. (MAIR-740, [#20](https://github.com/mairie360/Compliance_API/pull/20), MAIR-500, MAIR-771, [#23](https://github.com/mairie360/Compliance_API/pull/23))
- Image d'exécution passée en distroless Debian 13 (builder et `development.Dockerfile` en trixie), pour corriger la CVE-2026-84782 de `libssl3`. (MAIR-741, [#21](https://github.com/mairie360/Compliance_API/pull/21))
- CI : CICD passé à v4.3.1 (MAIR-571, [#15](https://github.com/mairie360/Compliance_API/pull/15)), puis à v5.2.2, qui ajoute le job `GDPR contract` (MAIR-771, [#23](https://github.com/mairie360/Compliance_API/pull/23)).
- Tests : les tests d'intégration tournent avec le rôle Postgres `compliance_api` et `mairie360_api_lib` 4.2.0 (MAIR-556, [#13](https://github.com/mairie360/Compliance_API/pull/13)). Ajout de tests de cohérence des workflows et du Dockerfile.

## Différence dev → staging (en dev, pas encore en staging)

#### Nouveautés
- `POST /api/v1/scans` lance le scan de la base et, si `REDIS_SCAN_URL` est défini, celui des clés Redis sans TTL ou à TTL supérieur à `REDIS_LONG_TTL_SECONDS` (30 jours par défaut). Il renvoie les constats (nombre et emplacement, jamais une valeur). Réservé au jeton de service (HS256, `role: service`). Le scan tourne aussi toutes les `SCAN_INTERVAL_SECONDS` (6 h par défaut, `0` désactive). (MAIR-498, [#5](https://github.com/mairie360/Compliance_API/pull/5), [#7](https://github.com/mairie360/Compliance_API/pull/7))
- `POST /api/v1/erasures/{userId}` démarre ou reprend l'effacement d'un compte : 202 avec les étapes, 404 si le compte est inconnu, 500 si l'effacement ne peut pas être enregistré. Ordre des étapes : compte Keycloak et contact Resend, puis `anonymize_user` en base, puis objets S3 `users/{user_id}/`, clés Redis et clé de sauvegarde. Chaque étape est journalisée sans donnée et relancée en arrière-plan (`ERASURE_RETRY_SECONDS`, 300 s par défaut). (MAIR-498, [#6](https://github.com/mairie360/Compliance_API/pull/6))
- `GET /api/v1/erasures/{userId}` renvoie les étapes de l'effacement et leur statut ; 404 si aucun effacement n'a été demandé pour ce compte. (MAIR-498, [#6](https://github.com/mairie360/Compliance_API/pull/6))
- `GET /api/v1/masking-patterns` (jeton de service) renvoie les motifs de masquage des logs : e-mail, téléphone français, JWT, bearer, hash argon2. Les adresses IP ne sont pas masquées. (MAIR-498, [#7](https://github.com/mairie360/Compliance_API/pull/7))

#### Changement de comportement
- Les routes `/api` n'acceptent plus le JWT des agents : seul un jeton HS256 signé avec `JWT_SECRET` et portant `role: service` passe, un jeton d'agent reçoit 401. Le `JwtMiddleware` de la lib n'est plus monté. (MAIR-498, [#5](https://github.com/mairie360/Compliance_API/pull/5))
- `POST /api/v1/scans` exécute désormais aussi le scan Redis quand `REDIS_SCAN_URL` est défini, et ses constats Redis s'ajoutent à la réponse. (MAIR-498, [#7](https://github.com/mairie360/Compliance_API/pull/7))
- Étape « clé de sauvegarde » de l'effacement : elle était toujours marquée « not configured on this instance ». Avec `SCW_SECRET_KEY`, `SCW_DEFAULT_PROJECT_ID` et `SCW_REGION`, elle détruit désormais la clé de l'utilisateur dans Scaleway Key Manager. (MAIR-500, [#9](https://github.com/mairie360/Compliance_API/pull/9))
- Sauvegardes : le binaire `compliance-backup` (scellement et restauration) est ajouté à l'image. Les données personnelles des sauvegardes sont chiffrées par utilisateur (AES-256-GCM) ; une sauvegarde dont la clé a été détruite restaure cet utilisateur anonymisé. (MAIR-500, [#9](https://github.com/mairie360/Compliance_API/pull/9))

#### Technique
- Squelette initial du service (commit `69d3fcb`, sans PR) puis adaptation depuis `API_template` : crate `compliance_api`, port 3004, Renovate standard, compose et README. (MAIR-498, [#2](https://github.com/mairie360/Compliance_API/pull/2))
- Dépendances : `cargo audit` corrigé (`Cargo.lock`, ignore de RUSTSEC-2026-0387 documenté), puis `zip` passé en 9.2.2 via `utoipa-swagger-ui`. Ajout de `aes-gcm`, `sqlx` et `testcontainers` pour le scellement. (MAIR-740, [#20](https://github.com/mairie360/Compliance_API/pull/20), MAIR-500, MAIR-771, [#23](https://github.com/mairie360/Compliance_API/pull/23))
- Image d'exécution passée en distroless Debian 13 (builder et `development.Dockerfile` en trixie), pour corriger la CVE-2026-84782 de `libssl3`. (MAIR-741, [#21](https://github.com/mairie360/Compliance_API/pull/21))
- CI : CICD passé à v4.3.1 (MAIR-571, [#15](https://github.com/mairie360/Compliance_API/pull/15)), puis à v5.2.2, qui ajoute le job `GDPR contract` (MAIR-771, [#23](https://github.com/mairie360/Compliance_API/pull/23)).
- Tests : les tests d'intégration tournent avec le rôle Postgres `compliance_api` et `mairie360_api_lib` 4.2.0 (MAIR-556, [#13](https://github.com/mairie360/Compliance_API/pull/13)). Ajout de tests de cohérence des workflows et du Dockerfile.

## Différence staging → prod (en staging, pas encore en prod)

_Rien._

## Historique des passages

<!-- release-journal-history:start -->
| Date | Environnement | Version | Commit | Run |
| --- | --- | --- | --- | --- |
| 2026-11-02 23:05 UTC | dev | — | `e9b68a0` | [run](https://github.com/mairie360/Compliance_API/actions/runs/41540046610) |
| 2026-11-01 12:59 UTC | dev | — | `2ec813b` | [run](https://github.com/mairie360/Compliance_API/actions/runs/41308132640) |
<!-- release-journal-history:end -->
