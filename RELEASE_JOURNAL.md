# Journal des releases — Compliance_API

<!-- release-journal-state {"dev": "3a341d5a8eeb7962187c646d1c7ffd7ef337d27a", "prod": "", "prod_tag": "", "staging": "", "updated": {"dev": "2026-11-03 01:32 UTC"}} -->
> Tenu à jour par la CI (`mairie360/CICD`, action `release/release-journal`) à chaque passage en dev, staging et prod. Ne pas modifier à la main : le fichier est réécrit au passage suivant.

| Environnement | Version | Commit | Mis à jour |
| --- | --- | --- | --- |
| prod | — | — | — |
| staging | — | — | — |
| dev | — | `3a341d5` | 2026-11-03 01:32 UTC |

## Ajouté sur main depuis la dernière release prod (aucune release prod)

#### Nouveautés
- `POST /api/v1/scans` lance le scan de la base, puis, si `REDIS_SCAN_URL` est défini, celui des clés Redis sans TTL ou à TTL supérieur à `REDIS_LONG_TTL_SECONDS` (30 jours par défaut). Il renvoie les constats (compteurs et emplacements, jamais de valeur) et tourne aussi toutes les `SCAN_INTERVAL_SECONDS` (6 h par défaut, `0` désactive). (MAIR-498, [#5](https://github.com/mairie360/Compliance_API/pull/5), [#7](https://github.com/mairie360/Compliance_API/pull/7))
- `GET /api/v1/masking-patterns` renvoie les motifs de masquage des logs (e-mail, téléphone français, JWT, bearer, hash argon2). L'adresse IP n'est pas masquée. (MAIR-498, [#7](https://github.com/mairie360/Compliance_API/pull/7))
- `POST /api/v1/erasures/{userId}` démarre ou reprend l'effacement d'un compte : 202 avec les étapes, 404 si le compte est inconnu. `GET /api/v1/erasures/{userId}` lit les étapes, 404 si aucun effacement n'a été demandé. L'ordre est Keycloak et Resend, puis la base (`anonymize_user`), puis S3, Redis et la clé de sauvegarde. Chaque étape est journalisée sans donnée et les échecs sont relancés en arrière-plan (`ERASURE_RETRY_SECONDS`, 300 s). (MAIR-498, [#6](https://github.com/mairie360/Compliance_API/pull/6))
- Sauvegardes chiffrées avec une clé par utilisateur. La clé est détruite à l'effacement, ce qui rend ses sauvegardes illisibles pour lui. Un binaire `compliance-backup` (seal / unseal) est livré dans l'image. La destruction n'a lieu que si Scaleway Key Manager est configuré (`SCW_SECRET_KEY`, `SCW_DEFAULT_PROJECT_ID`). (MAIR-500, [#9](https://github.com/mairie360/Compliance_API/pull/9))

#### Changements de comportement
- Tous les endpoints sous `/api` exigent désormais un jeton de service (`role: service`). Les jetons agents ne sont plus acceptés, et aucun compte n'est recherché : le `sub` désigne le service appelant. (MAIR-498, [#5](https://github.com/mairie360/Compliance_API/pull/5))
- Les jetons de service doivent être EdDSA, signés par la clé de Core (`JWT_PUBLIC_KEY`, `iss` `mairie360-core`, `aud` `mairie360`). Les jetons HS256 sont refusés (401) dès que `JWT_SECRET` n'est pas défini. Selon la PR, aucun service ne les appelle encore : un appelant doit obtenir un jeton de service émis par Core. (MAIR-680, [#25](https://github.com/mairie360/Compliance_API/pull/25))

#### Technique
- Import de `API_template` : crate `compliance_api`, image `compliance-api`, port 3004, Renovate standard. (MAIR-498, [#2](https://github.com/mairie360/Compliance_API/pull/2))
- CI : `mairie360/CICD` passe en v4.3.1 (MAIR-571, [#15](https://github.com/mairie360/Compliance_API/pull/15)), puis en v5.2.2 avec le contrôle RGPD, et un test vérifie que tous les workflows appellent la même version (MAIR-771, [#23](https://github.com/mairie360/Compliance_API/pull/23)).
- Tests d'intégration exécutés avec le rôle Postgres `compliance_api` au lieu du superutilisateur, `mairie360_api_lib` en 4.2.0 (MAIR-556, [#13](https://github.com/mairie360/Compliance_API/pull/13)).
- Image d'exécution et builder passés à Debian 13 (distroless `cc-debian13`), avec un test qui le vérifie (MAIR-741, [#21](https://github.com/mairie360/Compliance_API/pull/21)).
- Alertes `cargo audit` corrigées par mise à jour de dépendances, une exception documentée pour RUSTSEC-2026-0387 (MAIR-740, [#20](https://github.com/mairie360/Compliance_API/pull/20)).
- Tests : clés Ed25519 générées à chaque exécution (`test_secrets.sh`), plus aucun jeton ni clé committé ; `mairie360_api_lib` en 4.3.0 (MAIR-680, [#25](https://github.com/mairie360/Compliance_API/pull/25)).

## Différence dev → staging (en dev, pas encore en staging)

#### Nouveautés
- `POST /api/v1/scans` lance le scan de la base, puis, si `REDIS_SCAN_URL` est défini, celui des clés Redis sans TTL ou à TTL supérieur à `REDIS_LONG_TTL_SECONDS` (30 jours par défaut). Il renvoie les constats (compteurs et emplacements, jamais de valeur) et tourne aussi toutes les `SCAN_INTERVAL_SECONDS` (6 h par défaut, `0` désactive). (MAIR-498, [#5](https://github.com/mairie360/Compliance_API/pull/5), [#7](https://github.com/mairie360/Compliance_API/pull/7))
- `GET /api/v1/masking-patterns` renvoie les motifs de masquage des logs (e-mail, téléphone français, JWT, bearer, hash argon2). L'adresse IP n'est pas masquée. (MAIR-498, [#7](https://github.com/mairie360/Compliance_API/pull/7))
- `POST /api/v1/erasures/{userId}` démarre ou reprend l'effacement d'un compte : 202 avec les étapes, 404 si le compte est inconnu. `GET /api/v1/erasures/{userId}` lit les étapes, 404 si aucun effacement n'a été demandé. L'ordre est Keycloak et Resend, puis la base (`anonymize_user`), puis S3, Redis et la clé de sauvegarde. Chaque étape est journalisée sans donnée et les échecs sont relancés en arrière-plan (`ERASURE_RETRY_SECONDS`, 300 s). (MAIR-498, [#6](https://github.com/mairie360/Compliance_API/pull/6))
- Sauvegardes chiffrées avec une clé par utilisateur. La clé est détruite à l'effacement, ce qui rend ses sauvegardes illisibles pour lui. Un binaire `compliance-backup` (seal / unseal) est livré dans l'image. La destruction n'a lieu que si Scaleway Key Manager est configuré (`SCW_SECRET_KEY`, `SCW_DEFAULT_PROJECT_ID`). (MAIR-500, [#9](https://github.com/mairie360/Compliance_API/pull/9))

#### Changements de comportement
- Tous les endpoints sous `/api` exigent désormais un jeton de service (`role: service`). Les jetons agents ne sont plus acceptés, et aucun compte n'est recherché : le `sub` désigne le service appelant. (MAIR-498, [#5](https://github.com/mairie360/Compliance_API/pull/5))
- Les jetons de service doivent être EdDSA, signés par la clé de Core (`JWT_PUBLIC_KEY`, `iss` `mairie360-core`, `aud` `mairie360`). Les jetons HS256 sont refusés (401) dès que `JWT_SECRET` n'est pas défini. Selon la PR, aucun service ne les appelle encore : un appelant doit obtenir un jeton de service émis par Core. (MAIR-680, [#25](https://github.com/mairie360/Compliance_API/pull/25))

#### Technique
- Import de `API_template` : crate `compliance_api`, image `compliance-api`, port 3004, Renovate standard. (MAIR-498, [#2](https://github.com/mairie360/Compliance_API/pull/2))
- CI : `mairie360/CICD` passe en v4.3.1 (MAIR-571, [#15](https://github.com/mairie360/Compliance_API/pull/15)), puis en v5.2.2 avec le contrôle RGPD, et un test vérifie que tous les workflows appellent la même version (MAIR-771, [#23](https://github.com/mairie360/Compliance_API/pull/23)).
- Tests d'intégration exécutés avec le rôle Postgres `compliance_api` au lieu du superutilisateur, `mairie360_api_lib` en 4.2.0 (MAIR-556, [#13](https://github.com/mairie360/Compliance_API/pull/13)).
- Image d'exécution et builder passés à Debian 13 (distroless `cc-debian13`), avec un test qui le vérifie (MAIR-741, [#21](https://github.com/mairie360/Compliance_API/pull/21)).
- Alertes `cargo audit` corrigées par mise à jour de dépendances, une exception documentée pour RUSTSEC-2026-0387 (MAIR-740, [#20](https://github.com/mairie360/Compliance_API/pull/20)).
- Tests : clés Ed25519 générées à chaque exécution (`test_secrets.sh`), plus aucun jeton ni clé committé ; `mairie360_api_lib` en 4.3.0 (MAIR-680, [#25](https://github.com/mairie360/Compliance_API/pull/25)).

## Différence staging → prod (en staging, pas encore en prod)

_Rien._

## Historique des passages

<!-- release-journal-history:start -->
| Date | Environnement | Version | Commit | Run |
| --- | --- | --- | --- | --- |
| 2026-11-03 01:32 UTC | dev | — | `3a341d5` | [run](https://github.com/mairie360/Compliance_API/actions/runs/41559242197) |
| 2026-11-02 23:05 UTC | dev | — | `e9b68a0` | [run](https://github.com/mairie360/Compliance_API/actions/runs/41540046610) |
| 2026-11-01 12:59 UTC | dev | — | `2ec813b` | [run](https://github.com/mairie360/Compliance_API/actions/runs/41308132640) |
<!-- release-journal-history:end -->
