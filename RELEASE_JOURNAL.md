# Journal des releases — Compliance_API

<!-- release-journal-state {"dev": "5d350dc7afb31d562ec7be85bcd45654fe17efa5", "prod": "", "prod_tag": "", "staging": "", "updated": {"dev": "2026-11-13 01:12 UTC"}} -->
> Tenu à jour par la CI (`mairie360/CICD`, action `release/release-journal`) à chaque passage en dev, staging et prod. Ne pas modifier à la main : le fichier est réécrit au passage suivant.

| Environnement | Version | Commit | Mis à jour |
| --- | --- | --- | --- |
| prod | — | — | — |
| staging | — | — | — |
| dev | — | `5d350dc` | 2026-11-13 01:12 UTC |

## Ajouté sur main depuis la dernière release prod (aucune release prod)

- ci: MAIR-1003 Release prod: align internal versions and deploy ([#32](https://github.com/mairie360/Compliance_API/pull/32))
- fix(deps): MAIR-816 Release dev: align internal versions and deploy (#30) ([#30](https://github.com/mairie360/Compliance_API/pull/30))
- fix(deps): MAIR-741 APIs: move the runtime image to distroless Debian 13 (CVE-2026-84782) (#28) ([#28](https://github.com/mairie360/Compliance_API/pull/28))
- fix(deps): MAIR-740 APIs: fix the cargo audit RustSec alerts (actix-http, libsqlite3-sys) (#26) ([#26](https://github.com/mairie360/Compliance_API/pull/26))
- fix: MAIR-680 [Audit 23/10] Toutes les APIs et BFFs : remplacer le secret JWT HS256 partagé par 12 services (A1) ([#25](https://github.com/mairie360/Compliance_API/pull/25))
- ci: MAIR-771 APIs, BFFs, Database : monter CICD en v5.2.2 pour activer les contrôles RGPD (#23) ([#23](https://github.com/mairie360/Compliance_API/pull/23))
- fix: MAIR-741 APIs : passer l'image d'exécution en distroless Debian 13 (CVE-2026-84782) (#21) ([#21](https://github.com/mairie360/Compliance_API/pull/21))
- fix(deps): MAIR-740 APIs : corriger les alertes RustSec de cargo audit (crypto-bigint, ctutils, zstd) (#20) ([#20](https://github.com/mairie360/Compliance_API/pull/20))
- feat: MAIR-500 Chiffrer les sauvegardes avec une clé par utilisateur, détruite à son effacement (#9) ([#9](https://github.com/mairie360/Compliance_API/pull/9))
- feat: MAIR-498 Service de conformité de l'instance : scan permanent, effacement centralisé et journal des fuites (#7) ([#7](https://github.com/mairie360/Compliance_API/pull/7))
- feat: MAIR-498 Service de conformité de l'instance : scan permanent, effacement centralisé et journal des fuites (#6) ([#6](https://github.com/mairie360/Compliance_API/pull/6))
- feat: MAIR-498 Service de conformité de l'instance : scan permanent, effacement centralisé et journal des fuites (#5) ([#5](https://github.com/mairie360/Compliance_API/pull/5))
- feat: MAIR-498 Service de conformité de l'instance : scan permanent, effacement centralisé et journal des fuites (#2) ([#2](https://github.com/mairie360/Compliance_API/pull/2))
- chore(deps): MAIR-571 Bump CICD to v4.3.1 (#15) ([#15](https://github.com/mairie360/Compliance_API/pull/15))
- feat: MAIR-556 Exécuter les tests des APIs avec le rôle Postgres de l'API au lieu du superuser (#13) ([#13](https://github.com/mairie360/Compliance_API/pull/13))
- Initial commit ([`69d3fcb`](https://github.com/mairie360/Compliance_API/commit/69d3fcbad47bf597b090c1624510265de9cb4c59))

## Différence dev → staging (en dev, pas encore en staging)

- ci: MAIR-1003 Release prod: align internal versions and deploy ([#32](https://github.com/mairie360/Compliance_API/pull/32))
- fix(deps): MAIR-816 Release dev: align internal versions and deploy (#30) ([#30](https://github.com/mairie360/Compliance_API/pull/30))
- fix(deps): MAIR-741 APIs: move the runtime image to distroless Debian 13 (CVE-2026-84782) (#28) ([#28](https://github.com/mairie360/Compliance_API/pull/28))
- fix(deps): MAIR-740 APIs: fix the cargo audit RustSec alerts (actix-http, libsqlite3-sys) (#26) ([#26](https://github.com/mairie360/Compliance_API/pull/26))
- fix: MAIR-680 [Audit 23/10] Toutes les APIs et BFFs : remplacer le secret JWT HS256 partagé par 12 services (A1) ([#25](https://github.com/mairie360/Compliance_API/pull/25))
- ci: MAIR-771 APIs, BFFs, Database : monter CICD en v5.2.2 pour activer les contrôles RGPD (#23) ([#23](https://github.com/mairie360/Compliance_API/pull/23))
- fix: MAIR-741 APIs : passer l'image d'exécution en distroless Debian 13 (CVE-2026-84782) (#21) ([#21](https://github.com/mairie360/Compliance_API/pull/21))
- fix(deps): MAIR-740 APIs : corriger les alertes RustSec de cargo audit (crypto-bigint, ctutils, zstd) (#20) ([#20](https://github.com/mairie360/Compliance_API/pull/20))
- feat: MAIR-500 Chiffrer les sauvegardes avec une clé par utilisateur, détruite à son effacement (#9) ([#9](https://github.com/mairie360/Compliance_API/pull/9))
- feat: MAIR-498 Service de conformité de l'instance : scan permanent, effacement centralisé et journal des fuites (#7) ([#7](https://github.com/mairie360/Compliance_API/pull/7))
- feat: MAIR-498 Service de conformité de l'instance : scan permanent, effacement centralisé et journal des fuites (#6) ([#6](https://github.com/mairie360/Compliance_API/pull/6))
- feat: MAIR-498 Service de conformité de l'instance : scan permanent, effacement centralisé et journal des fuites (#5) ([#5](https://github.com/mairie360/Compliance_API/pull/5))
- feat: MAIR-498 Service de conformité de l'instance : scan permanent, effacement centralisé et journal des fuites (#2) ([#2](https://github.com/mairie360/Compliance_API/pull/2))
- chore(deps): MAIR-571 Bump CICD to v4.3.1 (#15) ([#15](https://github.com/mairie360/Compliance_API/pull/15))
- feat: MAIR-556 Exécuter les tests des APIs avec le rôle Postgres de l'API au lieu du superuser (#13) ([#13](https://github.com/mairie360/Compliance_API/pull/13))
- Initial commit ([`69d3fcb`](https://github.com/mairie360/Compliance_API/commit/69d3fcbad47bf597b090c1624510265de9cb4c59))

## Différence staging → prod (en staging, pas encore en prod)

_Rien._

> Résumé Claude indisponible pour ce passage : listes brutes des commits.

## Historique des passages

<!-- release-journal-history:start -->
| Date | Environnement | Version | Commit | Run |
| --- | --- | --- | --- | --- |
| 2026-11-13 01:12 UTC | dev | — | `5d350dc` | [run](https://github.com/mairie360/Compliance_API/actions/runs/43341212453) |
| 2026-11-12 13:08 UTC | dev | — | `7c635f6` | [run](https://github.com/mairie360/Compliance_API/actions/runs/43222221211) |
| 2026-11-12 11:51 UTC | dev | — | `645bb5c` | [run](https://github.com/mairie360/Compliance_API/actions/runs/43211700021) |
| 2026-11-03 01:32 UTC | dev | — | `3a341d5` | [run](https://github.com/mairie360/Compliance_API/actions/runs/41559242197) |
| 2026-11-02 23:05 UTC | dev | — | `e9b68a0` | [run](https://github.com/mairie360/Compliance_API/actions/runs/41540046610) |
| 2026-11-01 12:59 UTC | dev | — | `2ec813b` | [run](https://github.com/mairie360/Compliance_API/actions/runs/41308132640) |
<!-- release-journal-history:end -->
