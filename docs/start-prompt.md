# Start prompt

Bootstrap prompt to hand to a **fresh session** to execute the rebuild. It just points at
[`rebuild-prompt.md`](./rebuild-prompt.md) (the real prompt) and its two companion docs.

Use a **dedicated clone** — the rebuild wipes the existing code, so never run it in a working copy
you want to keep.

```
You are starting fresh, in a NEW clone of BaconDroid/test-checker:
  git clone https://github.com/BaconDroid/test-checker.git && cd test-checker

Read and execute docs/rebuild-prompt.md — it is the full implementation prompt.

First read it, then also read the two companion docs it references:
- docs/test-signal-research.md  (the signal catalog / research)
- docs/source-exploration.md    (verified sources + corrections)

SAFETY: the prompt tells you to WIPE the existing src/, tests/, benches/, npm/, .github/,
Cargo.toml, etc. and rebuild from scratch. Do this only inside this dedicated clone — never in
anyone's local working copy. Do all work on new branches and land every change via
branch → PR → (squash) merge with CI green; never force-push main.

Follow the prompt's workflow: read BaconDroid/comment-checker in full as the base, rebuild
test-checker per the prompt in small verified steps, and report progress per step with the exact
commands and their results.
```
