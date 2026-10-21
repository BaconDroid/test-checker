# Lean metrics — replay harness and the data it needs

The product goal is "lean, not zero": fewer narration comments and fewer scaffolding tests, without
weakening integrity. This harness measures that goal on **real AI-generated task diffs**.

**No corpus is shipped.** This file is the handoff note: it defines the schema and lists the data
needed to produce a meaningful before/after comparison.

## What the harness reports

Per group (`before` = hooks disabled, `after` = hooks enabled):

- **comments added per task** — count of added comment lines (heuristic: a trimmed line starting with
  `#`, `//`, `/*`, `*`, or `--`);
- **tests added per task** — count of added test declarations (`def test…`, `func Test…`, `it(…)`,
  `test(…)`, `#[test]` functions, `void`/`fun` methods);
- **block rate** — fraction of tasks the hooks would block (exit 2), only when `--cc`/`--tc` binaries
  are supplied;
- **agent revisions per task** — taken from the corpus (`revisions`), i.e. how many times the agent
  revised after an exit 2;
- **false-positive rate on kept items** — `kept_fp / kept` from human labels.

Fields without data are reported as `null`; nothing is inferred.

## Schema

```json
{
  "tasks": [
    {
      "task_id": "t1",
      "hook_enabled": true,
      "files": [
        {
          "path": "src/a.py",
          "old": "…",
          "new": "…",
          "revisions": 1,
          "kept": 3,
          "kept_fp": 0
        }
      ]
    }
  ]
}
```

`old`/`new` are the file before and after the agent's change. `revisions`, `kept` and `kept_fp` are
optional and belong to real trace data.

## Data needed

1. **Real AI task traces** — the prompt plus the resulting diffs, per task, anonymized if needed.
   Ideally across several repos and languages, and split into hook-off and hook-on runs.
2. **Revision counts** — for each task, how many times the agent revised after a hook exit 2. This
   requires recording the hook's verdict and the agent's next action.
3. **Human labels on kept comments/tests** — for each kept item, whether it was a true guard/intent
   (correctly kept) or a false positive (should have been flagged).
4. Optionally, the **task outcome** (tests passing, human review verdict) so "lean" can be related to
   quality, not just volume.

Without 1–3 the harness still runs and reports counts; it simply leaves the trace-dependent metrics
`null`.

## Run

```bash
python3 eval/lean_metrics.py --corpus tasks.json
python3 eval/lean_metrics.py --corpus tasks.json \
  --cc target/release/comment-checker \
  --tc ../test-checker/target/release/test-checker
```

The harness is read-only: it never writes files or mutates a repository.
