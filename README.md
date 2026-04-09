# Connect 4 Knowledge

`connect4_knowledge` is a Rust Connect 4 project built around knowledge-based play inspired by Victor Allis's thesis, available from [tromp.github.io](https://tromp.github.io/c4/connect4_thesis.pdf).

It combines:
- a rule-based evaluator for classic Connect 4 strategy patterns,
- bounded exact search for closure,
- a persistent solved-position book,
- an offline book expander,
- targeted position certification with blocker-following,
- and a small desktop GUI with falling-piece animation.

This is not yet a fully proven perfect 7x6 player. It is a hybrid engine that can play well, certify many positions exactly, and grow a reusable book over time.

## Current Status

Implemented:
- board model and group analysis
- rule generators for:
  - `Claimeven`
  - `Baseinverse`
  - `Vertical`
  - `Before`
  - `Aftereven`
  - `Lowinverse`
  - `Highinverse`
  - `Baseclaim`
  - `Specialbefore`
- compatibility and cover solving
- bounded alpha-beta search with transposition caching
- persistent `redb` opening/endgame book in `book_cache.redb`
- persistent certification cache, certification frontier, and certification node-state in the same `redb` database
- resumable offline frontier expansion in `book_frontier.json`
- targeted branch expansion with per-branch frontier files
- desktop GUI via `eframe`
- GUI move tracing and terminal move logging
- `certify-position` exact-search command with shared node budget, progress bar, throughput, ETA, blocker-following, and automatic ancestor backtracking

Not yet complete:
- full proof that all standard 7x6 positions are solved correctly
- full thesis-grade search/database pipeline
- broad audit coverage of all rule semantics against the thesis

## Running

Build and launch the GUI:

```bash
cargo run
```

Or explicitly:

```bash
cargo run -- gui
```

CLI commands:

```bash
cargo run -- help
cargo run -- play
cargo run -- explain
cargo run -- analyze
cargo run -- dump-proof
cargo run -- expand-book
cargo run -- expand-branch
cargo run -- certify-position
cargo run -- report-book
```

Examples:

```bash
cargo run -- play d d c
cargo run -- explain d d c
cargo run -- analyze c d c d
cargo run -- expand-branch d d --batch-limit 10000 --reset-frontier
cargo run -- certify-position d d --max-nodes 2000000
cargo run --release -- certify-position d d --max-nodes 10000000 --follow-until-stall --stall-limit 3 --total-max-nodes 200000000
```

## Offline Book Expansion

The project can grow its persistent book offline:

```bash
cargo run -- expand-book 10000
```

Reset the frontier and restart expansion strategy from the root:

```bash
cargo run -- expand-book 1000 --reset-frontier
```

Target one specific branch instead of the global expansion frontier:

```bash
cargo run -- expand-branch d d --batch-limit 10000 --reset-frontier
```

Branch runs use separate resumable frontier files such as:
- `book_frontier_branch_dd.json`
- `book_frontier_branch_ddc.json`

Files:
- `book_cache.redb`: persistent solved-position book plus certification caches/frontiers
- `book_frontier.json`: resumable global expansion frontier
- `book_frontier_branch_*.json`: resumable branch-specific frontiers

The current expander:
- harvests verifier-solvable positions,
- stores certified results in the book,
- automatically reseeds broader opening exploration when the current deep frontier is drained,
- and supports targeted branch filling when you want to improve a specific opening line.

For long unattended runs:

```bash
nohup cargo run --release -- expand-book 1000000 > expand.log 2>&1 &
```

Progress is checkpointed during the run, not just at the end of the batch.

## Targeted Certification

`certify-position` tries to solve one exact position directly and write the result back into the persistent book:

```bash
cargo run -- certify-position d d --max-nodes 2000000
```

For long certification runs, prefer release mode:

```bash
cargo run --release -- certify-position d d --max-nodes 10000000
```

It uses:
- exact book entries as oracle leaves,
- verifier closure inside the endgame band,
- a shared global node budget,
- a persistent certification cache,
- a persistent certification frontier for unresolved nodes,
- and persistent node-state to resume partial closure work.

Useful options:

```bash
cargo run -- certify-position d d --max-nodes 2000000 --auto-double
cargo run -- certify-position d d --dump-root-state
cargo run -- certify-position d d --max-nodes 10000000 --follow-blockers --follow-steps 4
cargo run -- certify-position d d --max-nodes 10000000 --follow-until-stall --stall-limit 3 --total-max-nodes 200000000
```

During long runs, the command prints:
- percentage progress
- nodes visited vs budget
- nodes/sec
- elapsed time
- ETA
- certification/root-state summary at the end of each iteration
- cumulative `total_nodes_visited`
- `stall_count` when using `--follow-until-stall`

Repeated runs on the same root can reuse:
- exact solved subpositions from `certify_cache`
- unresolved search-boundary nodes from `certify_frontier`
- partial child-state from `certify_node_state`

`--dump-root-state` is a cheap inspection mode that does not run search. It prints:
- root-child exact/unresolved status
- attempted vs exact child counts
- root-child frontier counts and minimum empties
- per-grandchild statuses and pair-level frontier counts

`--follow-blockers` automates the manual blocker-seeking workflow:
- run `certify-position` on the current root
- inspect the blocker state
- descend to the dominant blocker pair
- when a branch certifies, backtrack automatically to the nearest unresolved ancestor

`--follow-until-stall` removes the need to guess a fixed number of follow steps. It keeps descending/backtracking until:
- the search stalls for `--stall-limit` consecutive iterations,
- `--total-max-nodes` is reached, if supplied,
- no blocker can be found,
- or the current line certifies and there is no unresolved ancestor left.

## Architecture

Key files:
- [src/policy.rs](/home/urban/Code/Rust/Connect4/src/policy.rs): move selection
- [src/verifier.rs](/home/urban/Code/Rust/Connect4/src/verifier.rs): bounded exact search
- [src/bookdb.rs](/home/urban/Code/Rust/Connect4/src/bookdb.rs): runtime `redb` book storage
- [src/book.rs](/home/urban/Code/Rust/Connect4/src/book.rs): JSON import/export compatibility layer
- [src/expander.rs](/home/urban/Code/Rust/Connect4/src/expander.rs): offline book construction and targeted certification
- [src/gui.rs](/home/urban/Code/Rust/Connect4/src/gui.rs): GUI
- [src/rules/](/home/urban/Code/Rust/Connect4/src/rules): rule generators

Move choice order:
1. persistent book lookup
2. immediate wins / blocks
3. bounded root search
4. rule-proof child selection
5. verifier-certified child selection
6. heuristic fallback

Current practical note:
- the engine is still not proven perfect
- the certification toolchain is now good enough to chase and solve narrow proven lines deeply
- long-running `certify-position --follow-until-stall` jobs are the main way the book is being strengthened in important opening branches

The GUI also exposes the basis for AI moves and keeps a short move trace so you can see when the engine is playing from:
- `Book`
- `Verifier`
- `RuleProof`
- `Search`
- `Heuristic`

## Development

Run tests:

```bash
cargo test
```

Run clippy:

```bash
cargo clippy --all-targets --all-features
```

The codebase convention for this project is to run clippy after any Rust file change.

## Source Material

Project planning documents:
- [PLAN.md](/home/urban/Code/Rust/Connect4/PLAN.md)
- [BUILD_SPEC.md](/home/urban/Code/Rust/Connect4/BUILD_SPEC.md)

Primary reference:
- [A Knowledge-based Approach of Connect-Four](https://tromp.github.io/c4/connect4_thesis.pdf)
