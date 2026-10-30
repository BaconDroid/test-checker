# Handoff — test-guard (test-checker / comment-checker / opencode-comments-plugin)

Snapshot du 2026-10-28, à jour après le merge du plugin #70. Objectif : reprendre le projet sur une autre machine/une autre session.

## 0. État des trois repos (GitHub `BaconDroid`)

| repo                      | défaut    | head       | PR ouvertes                                                              | issues ouvertes    |
| ------------------------- | --------- | ---------- | ------------------------------------------------------------------------ | ------------------ |
| `comment-checker` (cc)    | `master`  | `7a4972b`  | aucune                                                                   | aucune             |
| `test-checker` (tc)       | `main`    | `60e58e0b` | **#67** `tc/conditional-assertion-precision` (`8e2cdc6`, UNSTABLE), **#68** `tc/guard-thresholds` | —                  |
| `opencode-comments-plugin` | `master` | `a0e2e13`  | aucune                                                                   | **#71**, **#72** (ouvertes) |

Rappel : `delete_branch_on_merge=true` sur les 3 repos → les branches de PR sont supprimées au merge.
Protection de branche : checks requis (`rust`+`parity` pour cc/tc, `test` pour le plugin), `enforce_admins=true`, pas de force-push ni de suppression.

## 1. Travail en attente, par priorité

### T1 — PR #67 NON approuvée : corriger `conditional-assertion` (spec complète)

Raisons du refus : 29 payloads bloquants au gate défaut vs 19 sur main, 17/29 FP (dont 12 nouveaux à cause du garde Rust `if let Some` de `tests/parity.rs`), et Sourcery FAIL (les 3 findings visent `eval/guard_thresholds.py`, désormais corrigés dans #68).

À faire sur `tc/conditional-assertion-precision` (après rebase sur main une fois #68 mergé, en retirant `eval/guard_thresholds.py`, `eval/replay_removal_truth.json` et la section README déplacés) :

1. **Commit 1 — démettre `conditional-assertion` à Low** : confiance de la règle, CONCEPTS.md et table des règles du README doivent le dire. **Ne pas toucher au gate par défaut** (Low reste invisible au gate par défaut).
2. **Commit 2 — comportement plus étroit** :
   - ignorer un `if` dont la condition est une garde de pattern/option : Rust `if let Some(..)` / `if let Ok(..)`, Go `if err != nil` et `if x, ok := ...; ok`, JS `if (err)`, Python `isinstance` / `is not None` ;
   - détecter une garde ajoutée autour d'une assertion **pré-existante** (`assert x` → `if flag: assert x`) : le diff doit voir la ligne retirée ET la nouvelle ligne gardée ;
   - fixtures par langage (Go, TypeScript, Python, Rust, Java), y compris le garde exact `if let Some(needle) = ...` de `tests/parity.rs`.
3. **Replay** (même harnais, même corpus 320) : table 2×2 + table par règle (findings, payloads bloquants, FP avant/après) pour main `60e58e0` et la nouvelle tête. **Contrainte : payloads bloquants au gate défaut ≤ 19.**

### T2 — Merger PR #68 (`tc/guard-thresholds`) après review

Script de seuil déplacé + correctifs Sourcery (chemins `--repos` portables, dénominateur recall comptant tous les payloads de vérité, annotation `nosemgrep`), rejouable depuis le binaire livré via `TEST_CHECKER_GUARD_METRICS=1` (jamais actif par défaut).

### T3 — Corpus secondaire : approuver la proposition (rien lancé)

3 repos d'applications OSS, licence vérifiée, recette `Write` du contenu commité + `Edit` parent→commit par commit touchant un fichier de test :

| repo                    | langue | licence              | commits test | payloads | download              |
| ----------------------- | ------ | -------------------- | -----------: | -------: | --------------------- |
| `sharkdp/fd`              | Rust   | MIT OR Apache-2.0    |         ~225 |     ≥100 | clone ~3 Mo           |
| `netbox-community/netbox` | Python | Apache-2.0           |   80–870/app |  100–160 | clone ~224 Mo         |
| `calcom/cal.diy` (cal.com) | TS     | MIT                  |     819+309 |  100–120 | `--filter=blob:none`    |

Ordre conseillé : fd → netbox → cal.diy. Caveats : fd = diversité faible (`tests/tests.rs`) ; netbox = filtrer `test_*.py` ; cal.diy = ignorer artefacts Playwright/Prisma.

### T4 — Issues plugin à traiter

- **#71** Forward harness/config paths to test-checker (pour `config-edited` / `gate-weakened` sous OpenCode ; tc reconnaît les paths harness indépendamment du langage, cf. tc PR #32).
- **#72** Warn when the cached binary is older than the plugin expects (cache périmé `0.3.1` → fallback regex silencieux observé le 27/10).

### T5 — Phase « release readiness » (read-only, jamais exécutée)

Versions (Cargo.toml, package.json), déclencheurs des workflows de release (tag ? manuel ?), ce qu'un tag publie (GitHub release / npm / notarisation macOS / canary), existence d'un dry run, bump de version + changelog, et ce qui rendrait une release dangereuse aujourd'hui (ex. `eval/baseline.json` périmé après rebase — outil manuel, hors release). Confirmer aussi que cc et tc partagent le contrat d'exit codes **0 / 2 / 1** et que le plugin traite le exit 1 comme « did not run ».

## 2. Procédures (à respecter telles quelles)

**Merge (ROLE RULES)** : avant tout merge, cloner la tête de la PR dans un clone FRAIS, builder, re-exécuter les commandes d'acceptance de la phase depuis ce clone et coller les sorties ; si divergence → STOP. Vérifier `headRefOid`, les checks verts sur ce SHA exact (`rust`/`parity` pour cc/tc, `test` pour le plugin ; Sourcery peut être SKIPPED mais jamais FAIL/PENDING), `gh pr diff --name-only` conforme ; merger avec `gh pr merge <n> --match-head-commit <sha> --merge` (cc/tc) ou `--squash` (plugin) ; pas de force-push ; ne JAMAIS résoudre un conflit pendant un merge ; ne merger que la PR nommée. Après merge : CI de la branche par défaut verte, parité dans les deux sens depuis des clones frais de cc master et tc main (`PARITY_SIBLING=<autre clone> python3 parity/check.py`), `PARITY_REF` résolu sur la branche par défaut, liste des PR devenues conflictuelles. Jamais de tag/release ni de suppression de branche hors phase de cleanup. Si un merge tourne mal : ne pas revert seul, rapporter et attendre.

**Gates** : `cargo fmt --all -- --check` ; `cargo clippy -p test-checker --all-targets --all-features -- -D warnings` ; `cargo test -p test-checker --all-targets --all-features` ; `node --test npm/test-checker/*.test.js` ; parité dans les deux sens ; plugin : `bun test` (+`TC_BINARY`, `TC_REPO`), `bun run typecheck`, `bun run build`, `npm run check:rule-ids`.

**Sémantique du produit** : gate par défaut `--min-confidence medium` (les findings Low sont invisibles) ; exit codes 0 (clean/skip) / 2 (findings) / 1 (did not run) ; baseline write opt-in `TEST_CHECKER_WRITE_BASELINE` (diff vs HEAD) ; `.test-checker.json` lu **uniquement à la racine git** ; markers `test-checker: allow <rule> -- <reason>=12+` uniquement dans les commentaires ; `config-edited` High ; hooks = « speed bump », pas une frontière de sécurité.

**Replay / métriques** : `eval/replay.py` (320 payloads = 2 repos × 80 commits × 2 kinds) ; vérité des suppressions dans `eval/replay_removal_truth.json` (12 cas réels, SHA dans `eval/corpus.json`) ; `eval/guard_thresholds.py` (seuils 20/40/60 %, rejouable via `TEST_CHECKER_GUARD_METRICS=1`) ; rappel ≥1.0 pour les règles de suppression dans `tests/integration.rs`.

## 3. Savoir-faire machine (l'environnement actuel)

- `/tmp` est un tmpfs avec quota plein → `export TMPDIR=/var/home/bacon/tmp` (sinon échecs EDQUOT et tests cassés).
- Un `CARGO_TARGET_DIR` par build sous `/var/home/bacon/targets/…` (les builds propres prennent ~6-8 min).
- `git` : `worktree remove --force`, `checkout --ours`, `rebase --abort`, `rm -rf` sont bloqués par un safety net local ; résoudre les conflits à la main.
- Le test `detector::tests::masked_view_work_is_bounded_and_linear` est déterministe grâce au seam `IGNORE_PARSE_BUDGET` (`#[cfg(test)]`) ; le budget production reste 50 ms.
- Le plugin résout ses binaires via `$XDG_CACHE_HOME/opencode-comments-plugin/…` (**jamais** `PATH`) ; le wrapper hook du README tc utilise `PATH`. Un cache périmé dégrade silencieusement vers les règles regex (issue #72).
- Token GitHub actuel : classique très large. À remplacer par un token fine-grained limité aux 3 repos (Pull requests: write, Contents: write, **sans admin**).

## 4. Artefacts pratiques (sur cette machine, `/var/home/bacon/tmp`)

- `replay-verify66.py` — patron de replay 2×2 (import `eval/replay.py`, binaires, gates default/low).
- `harness-70.ts` — scénarios plugin contre binaire réel (`XDG_CACHE_HOME` + plant du binaire ; **toujours** exporter `XDG_CACHE_HOME`).
- `acceptance-raw.py` / `acceptance-raw.out` — 15 lignes d'acceptance marker/config en brut.
- `recall-66.py` — vérifie les 12 suppressions réelles au gate défaut.
- Worktrees : `wt-cond`, `wt-guard`, `wt-removal`, `wt-marker`, `wt-mask`, `wt-handoff` ; binaires dans `/var/home/bacon/targets/*`.

## 5. Historique récent (tout est mergé sauf #67/#68)

tc : #57 (exit 1 interne), #59 (baseline write opt-in), #61 (masked view + test de travail borné), #62 (markers ligne/config + multiset assertion-removed), #66 (précision des suppressions + garde refactor + corpus de rappel).
plugin : #69 (warning did-not-run), #70 (contrat rule-ids + aliases + filtre Low, squash `a0e2e13`).
cc : #21/#22/#23 (parité + ratio gate, documents).
