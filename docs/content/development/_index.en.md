+++
title = "Development Documentation"
weight = 30
sort_by = "weight"
[extra]
source_file_hash = "8b3543f7c59b7f5c79e42f26ba37e13c24f607cc"
translated_at = "2026-10-25T00:00:00Z"
+++

# Development Documentation

System documentation organised by "which part you need to touch": it describes **what is true now**. How it became that way, and what was rejected, lives in the [development records](@/records/_index.en.md).

Reading path:

1. [Server](@/development/server/_index.en.md) — core architecture first, then the pipeline and dialogue flow, then protocol and model deployment as needed.
2. [Clients](@/development/clients/_index.en.md) — shared semantics live in one place; everything else is per platform.
3. [IoT Firmware](@/development/iot/_index.en.md) — layers first, then feature criteria, then flashing and each path's semantics.

As needed:

- [Debugging](@/development/debugging/_index.en.md) — ASR benchmarks and debugging entry points, TTS audio quality evaluation
- [Model Downloader](@/development/downloader.en.md) — manifests, downloads, verification
- [Related Projects](@/development/related-project/_index.en.md) — call chains in upstream implementations worth comparing against
- [Resources](@/development/resource/_index.en.md) — measured properties of the test samples

## Multilingual Documentation

After editing a Chinese page under `docs/`, translate it to `.en.md` and run `git hash-object <source.md>` to update the `source_file_hash` and `translated_at` in its front matter.
