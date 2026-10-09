# T1170: the stale-upstream push trap, and the `push-guard` in front of it

Task: #1170 (`HOST-PUSH-DEFAULT-GUARD`). Date: 2026-10-09. This finding
records what was measured about the #1155 incident's mechanism, what the
guard does with it, and the AGENTS.md text the owner is asked to add (the
file is protected, so it cannot be edited from this task).

## What happened (#1155, repaired within ~3 minutes)

A `git push -u origin <task-branch>` from this shared host pushed the task
branch's commits to `refs/heads/main` on the remote instead of creating the
task branch. The push was reversed with a leased force-push of main back to
its exact pre-push SHA and re-sent as an explicit
`HEAD:refs/heads/<branch>` push.

## Measured mechanism (git 2.50.1, Apple Git-155, this host)

Reproduced in a scratch repository before the guard was written, and pinned
as a permanent regression by
`tools/cs_xtask/tests/accept_t1170_push_guard.rs::accept_t1170_the_unguarded_push_really_updates_main`:

1. `git checkout -qb rally/x origin/main` records
   `branch.rally/x.remote = origin`, `branch.rally/x.merge = refs/heads/main`
   (git's automatic upstream setup).
2. A later `git checkout --no-track -B rally/x …` keeps that config:
   `--no-track` only skips setting up a *new* upstream; it never removes an
   existing one.
3. `push.default = upstream` (the repo-wide value on this host when the
   incident happened) makes git **rewrite the push destination** from that
   merge ref. Measured outcomes with `branch.<name>.merge = refs/heads/main`:

   | `push.default` | command                          | destination pushed   |
   | -------------- | -------------------------------- | -------------------- |
   | `upstream`     | `git push -u origin <branch>`    | **`main`** (the trap) |
   | `upstream`     | `git push -u origin`             | **`main`** (the trap) |
   | `upstream`     | `git push -u origin HEAD:refs/heads/<branch>` | `<branch>` |
   | `current`      | `git push -u origin <branch>`    | `<branch>`            |
   | `simple`       | `git push -u origin <branch>`    | `<branch>`            |

   (`simple` additionally makes a *bare* `git push` refused by git when the
   upstream name differs from the branch name; that refusal is git's, and
   the guard surfaces it as a note.)

The shared checkout's `.git/config` (the `rust-crimson-skies/github` repo)
now carries `push.default = current`, which makes the rewrite inert — but
every fresh worktree, every cloned repo and every future host starts from
git's defaults again, so the configuration alone is not the guard.

## The guard: `cs_xtask push-guard`

`tools/cs_xtask/src/push_guard.rs`, wired into the `cs_xtask` binary. Run it
(as agents already run `verify-target-dir`) before pushing:

```sh
cargo run -p cs_xtask -- push-guard          # judge the checked-out branch
cargo run -p cs_xtask -- push-guard --branch rally/<name>
```

* It reads every fact from git itself (`symbolic-ref`, `config --get`,
  `for-each-ref`) and resolves the destinations of the two forms agents use
  — bare `git push` and `git push [-u] [remote] <branch>` — under the
  effective `push.default`. It never talks to the network: "does the remote
  already have this branch" is not needed to refuse a push at main.
* It prints each destination ref (`a push would update: refs/heads/…`) and
  exits 1, naming the fixes, when any destination is `refs/heads/main`
  (or when the configuration cannot be resolved at all — detached HEAD,
  unknown `push.default`, `upstream` with no merge ref — so that "unknown"
  never passes as "safe").
* `matching` cannot be resolved offline (its destinations are the branches
  that exist on both sides), so every local branch is treated as a
  candidate destination; a local `main` is then a refusal.

## Proposed AGENTS.md text (owner edit required; AGENTS.md is protected)

> **Before every `git push` from a task branch**, run
> `cargo run -p cs_xtask -- push-guard` and push only when it passes. It
> resolves — offline, from git's own configuration — which remote refs the
> push would update, and refuses when any is `refs/heads/main`. The trap it
> guards: a branch taken from `origin/main` without `--no-track` records
> `branch.<name>.merge = refs/heads/main`; `git checkout --no-track -B` does
> not remove it; with `push.default = upstream` git rewrites
> `git push -u origin <branch>` to push to `main`. `git config push.default
> current` (or an explicit `HEAD:refs/heads/<branch>` refspec) does not
> rewrite. See `docs/findings/2026-10-09-t1170-push-guard-stale-upstream-merge-ref.md`.

## Residual risk (owner decision, not agent-fixable)

The guard is a mandatory local check, not a server-side wall: an agent (or
the owner) can still run plain `git push` without it, and nothing on GitHub
refuses a non-Rally push to main. The real wall is GitHub branch
protection on `main` (require PR / restrict pushes to the landing flow),
which only the repository owner can configure. Recommend enabling it once
Rally's landing flow's identity is known to GitHub.
