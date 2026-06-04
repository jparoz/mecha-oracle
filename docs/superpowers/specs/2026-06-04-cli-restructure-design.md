# CLI Restructure: Unified Binary with clap, tracing, and Subcommands

**Date:** 2026-06-04
**Status:** Approved

## Problem

Starting either binary prints one `eprintln!` warning per card that fails to parse. With the full Scryfall dataset, this floods the terminal with thousands of lines. The parser only understands a small keyword subset, so the failures are expected noise, not real errors.

Additionally, the project has two separate binaries (`mecha-oracle` and `ui`) with ad-hoc argument parsing, making the CLI surface inconsistent.

## Goals

1. Default output shows a single summary line (e.g., `Loaded 12345 cards, 4567 skipped`).
2. `--verbose`/`-v` shows each individual parse failure followed by the summary.
3. All CLI parsing uses `clap` with the derive API.
4. Both binaries merge into one `mecha-oracle` binary with two subcommands: `serve` and `demo`.

## Non-Goals

- Changes to the engine, types, parser, or card-loading logic beyond replacing the `eprintln!`.
- Changing the card database API (still returns `Result<Self, String>`).

## Dependencies

```toml
clap = { version = "4", features = ["derive"] }
tracing = "0.1"
tracing-subscriber = "0.3"
```

## CLI Shape

```
mecha-oracle [-v/--verbose] <SUBCOMMAND>

Subcommands:
  demo                           Run the Phase 1 demo game
  serve [--shuffle] <DECK.JSON>  Start the interactive UI server
  update-cards                   Download/update the card database
```

`--verbose`/`-v` is declared `global = true` on the `Cli` struct so it works with any subcommand.

`update-cards` is promoted from a `--flag` to a proper subcommand, consistent with the new structure.

## Clap Derive Structure

```rust
#[derive(Parser)]
#[command(name = "mecha-oracle", about = "MTG Rules Engine")]
struct Cli {
    #[arg(short, long, global = true, help = "Show per-card parse warnings")]
    verbose: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Demo,
    Serve {
        #[arg(long)]
        shuffle: bool,
        deck: String,
    },
    UpdateCards,
}
```

## Logging

Individual card parse failures log at `DEBUG` — they are routine and expected, not actionable warnings. The end-of-load summary logs at `INFO`.

| Level | What appears |
|-------|-------------|
| `INFO` (default) | `INFO card database loaded loaded=12345 skipped=4567` |
| `DEBUG` (verbose) | Per-card debug lines, then the INFO summary |

Subscriber is initialized in `main()` before any other work. Timestamps and module paths are suppressed so non-verbose output is a clean single line:

```rust
let level = if cli.verbose { Level::DEBUG } else { Level::INFO };
tracing_subscriber::fmt()
    .with_max_level(level)
    .without_time()
    .with_target(false)
    .init();
```

Example non-verbose output:
```
 INFO card database loaded loaded=12345 skipped=4567
```

Example verbose output (excerpt):
```
DEBUG skipped card card="Counterspell" error="unknown keyword \"counter target spell\""
DEBUG skipped card card="Lightning Bolt" error="unknown keyword \"deals 3 damage to any target\""
 INFO card database loaded loaded=12345 skipped=4567
```

## File Changes

### Modified

| File | Change |
|------|--------|
| `Cargo.toml` | Add `clap`, `tracing`, `tracing-subscriber`; remove `[[bin]]` entry for `ui` |
| `src/cards/mod.rs` | Replace `eprintln!` with `tracing::debug!`; add `loaded`/`skipped` counters; emit `tracing::info!` summary after loop |
| `src/main.rs` | Rewrite as clap CLI entry point; inline `demo` subcommand logic; delegate `serve` to `src/serve.rs` |

### Added

| File | Purpose |
|------|---------|
| `src/serve.rs` | Serve subcommand — all logic from `src/bin/ui.rs`, plus its tests |

### Deleted / Moved

| Old path | New path / fate |
|----------|----------------|
| `src/bin/ui.rs` | Deleted (content moves to `src/serve.rs`) |
| `src/bin/ui.html` | Moved to `src/serve.html`; `include_str!` path updated |

### Unchanged

- All modules under `src/engine/`, `src/types/`, `src/parser/`, `src/cards/scryfall.rs`
- Test fixtures in `tests/`
- Existing tests (move verbatim into `src/serve.rs` with no changes)

## Test Impact

Tests in `src/bin/ui.rs` move to a `#[cfg(test)] mod tests` block in `src/serve.rs`. The `test_db()` helper and all test functions are unchanged. No new tests are required — the refactor is structural.
