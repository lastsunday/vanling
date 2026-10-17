+++
title = "Check tasks and caching record"
weight = 80
sort_by = "weight"
[extra]
source_file_hash = "401cf8e79082b6eebf6e5aa4e2ef025315f3b806"
translated_at = "2026-10-17T08:56:39Z"
+++

<!-- doc-audience: ai -->

# Check tasks and caching record

`apps/iot`'s verification loop: which tasks really run every time, what goes into `moon`'s cache key, and why the C6 product check no longer uses a container. **AI output, not reviewed by a human**; see the [parent note](@/records/_index.en.md).

Every claim is marked: **`[test]`** has a unit test, **`[measured]`** has a measurement on hardware or on this machine plus how to reproduce it, **`[reported]`** was not reproduced and is only kept on record.

## In one line

Before, the model was "every `moon run` recompiles the whole matrix": `check-features` was `cache: false` and `check-c6` went through the container. Now the same tree replays in **0.09 s / 0.10 s**, while the run that actually changed a file still pays full price. **What is saved is "re-run although nothing changed" — no check was weakened.**

## What is in moon's cache key

`[measured]` `.moon/cache/hashes/<hash>.json` *is* the key, and it lists its parts: `command`/`script` (the task text, so editing a command in `moon.yml` invalidates), `args`, `env` (the project-level `env:`), `inputEnv`, `outputs`, `toolchains`, `version`, and `inputs` — **one sha1 per file**.

- **With no `inputs`, the default is "every file under this project"**: `check-host`'s default key holds 121 entries, including `.gitignore`, `assets/*.mid`, `partitions/*.csv`, `linker/*.x`, plus `.moon/workspace.yml` and `.moon/toolchains.yml`. `apps/iot/moon.yml` is **not** among them (the command text lives in the `script` entry, so editing a command still invalidates).
- **Declaring `inputs` replaces that default rather than adding to it.** Naming only the script drops every project file from the key.
- Relative paths resolve from the project root and `..` is not legal (moon rejects it outright); **a path starting with `/` resolves from the workspace root**.

`[measured]` `'**/*'` covers everything in the default's 121 entries except `.moon/*` (dotfiles, `.mid`, `.csv`, `.x` — not one missing) and adds `apps/iot/moon.yml` itself. `moon.yml`'s `fileGroups.build-inputs` is exactly that: `'**/*'` plus `'/scripts/iot-xtensa.sh'`.

### The blind spot: tasks that compile through a workspace-root script

`check-s3`, `check-features` and the seven `probe-*-s3` tasks all compile through `scripts/iot-xtensa.sh`, and that script lives outside the project, so it was not in the default hash: **editing the dispatcher did not invalidate the cache.** moon would replay a stale verdict, or leave a probe ELF on disk as a satisfied `output` that the dispatcher no longer describes.

`[measured]` Verified directly (using `check-host` as the target, with the script temporarily added to its `inputs`): the baseline key was `0973f854`; after appending one comment line to `scripts/iot-xtensa.sh` the task **ran** and the key became `577401ca`; after restoring a byte-identical file the key went back to `0973f854` and the task **hit the cache** — the script's content is in the key, and the key is content-addressed.

So those tasks now name their inputs explicitly (`@group(build-inputs)`, plus its own script for `check-features`). `check-c6` is not one of them: it no longer goes through the dispatcher.

## `check-features`: `cache: false` became a content hash

The matrix is 15 subsets (6 on the host, 9 against the bare-metal targets), and each subset is a feature combination cargo has never seen.

| Case              | Before                | After                                |
| ----------------- | --------------------- | ------------------------------------ |
| Nothing changed   | full price (`cache: false`) | **0.09 s** (replay, marked `cached`) |
| Source changed    | full price            | full price: **2m20s570ms**           |
| Only a script changed | full price        | **35 s** (cargo artifacts still fresh) |

`[measured]` A single S3 subset: **2.48 s** on a repeated run with warm artifacts, **14.84 s** the first time — so those ~15 s are real compiling, not dispatch overhead. Starting one `docker run` (both mounts, `-w`, `bash -lc`, `source export-esp.sh`) costs **0.45 s**. Those two minutes therefore buy cargo's work; a cache can only remove the "nothing changed" share of it.

## `check-c6`: container → host stable toolchain

It is now a plain host command (`--bin vanling --bin wifi-probe`; the two features are equivalent to the defaults), for three reasons:

1. `riscv32imac-unknown-none-elf` ships a prebuilt `core`/`alloc` (`rust-toolchain.toml` installs the target), so it needs neither the esp fork nor `-Z build-std`;
2. the C6's **firmware is produced on the host's stable toolchain anyway** (`build` → `image-c6` both run there), so the container guaranteed nothing extra about the image coming out;
3. `[measured]` **1m12s** (container, earlier reading) → **1.46 s of cargo / 1.654 s of task** (warm artifacts).

`[measured]` Running `check` right after `check-c6` took 0.19 s (artifacts fresh), so the two share one set of artifacts and do not thrash each other. **The S3 keeps the container**: Xtensa has no prebuilt rust-std at all.

## Rejected: merging the 8 `docker run`s into 1

This was on the plan, on the grounds that "every `docker run` has a fixed cost". **The measurement killed the grounds**: starting one costs 0.45 s, not 5.4 s (the 5.4 s reading came from a cold container/cold VM; re-running the same command three times gave 0.45 s each). So merging would save at most **3.5 s**, at the price of a batch protocol in `iot-xtensa.sh` (cargo command lines over stdin, attributing failures, offline-retry semantics). Not worth it; not done.

## The guard that stays: the C6 subset still goes through the dispatcher

In `iot-xtensa.sh`, `ESP_HAL_CONFIG_DATA_CACHE_SIZE` is S3-only, and exporting it unconditionally breaks C6 builds (esp-config rejects an option that is inactive for the chip). CI's `check-c6` used to hold that gating honest; with `check-c6` on the host, **the C6 subset in `check-features` is now the only task handing the dispatcher a non-S3 target**, so that subset deliberately stays as it is: it buys this particular check nothing (one more container) but it is the gating's only witness in CI.

## Open

- **Splitting `check-s3` into product and probe tasks**: the gain is unmeasured, and it would shake the existing "all five bins are compiled on every `check-s3`" anti-rot line (see the "Probes" section of the [index](@/records/iot/_index.en.md)).
- **`smoke-host` is cached now** (15 s → **0.084 s** replay): its output is a log, and a replay carries the `cached` marker; use `moon run --force` to actually run it.
- **sccache / `target` slimming, Docker VM memory or `CARGO_BUILD_JOBS`, test-profile debuginfo**: all three can only be measured from a genuinely dirty state (change one core file → `cargo test -p iot-core` is 51 s), and none was measured this round.
- Machine side: 13 GB free disk / 8 GB RAM, with `apps/iot/target` at 8.6 G, the root `target` at 3.5 G and sccache at 11 G. Running the eight S3 subsets in parallel would need a target directory each (cargo locks one target directory), which does not fit — that is the machine-side reason the matrix cannot go faster.
