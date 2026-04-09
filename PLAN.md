# Connect 4 Knowledge-First Engine Plan

## Goal

Build a Connect 4 player whose primary decision system is a knowledge-based prover derived from Victor Allis's thesis, using proven strategic rules and threat reasoning instead of broad brute-force search. The target outcome is:

- perfect play on the standard 7x6 board,
- real-time move selection during gameplay,
- explainable move choices backed by named strategic rules,
- limited tactical verification only where the rule system cannot fully resolve a position.

## Guiding Position

The thesis does not support "perfect play from heuristics alone" in the strict sense. It supports:

- a knowledge-first evaluator based on nine proven rules,
- exact resolution of many positions without tree search,
- a narrow search layer and opening database to close the remaining gaps on 7x6.

This build should preserve that structure:

- gameplay path: rule engine first, book second, verifier last,
- no full-width brute-force minimax as the primary engine,
- any search that exists must be small, targeted, and offline-certifying rather than the default play policy.

## Scope

In scope:

- standard 7x6 Connect 4,
- the nine formal rules from the thesis,
- threat and threat-combination analysis,
- rule-combination solver,
- canonical board representation and symmetry reduction,
- optional certified opening/endgame knowledge store,
- explainable move selection.

Out of scope for v1:

- Monte Carlo methods,
- generic alpha-beta engine as the main player,
- neural evaluation,
- full retrograde tablebase generation,
- variants beyond rectangular even-height boards except where representation naturally generalizes.

## Delivery Phases

### Phase 1: Thesis Extraction and Rule Catalog

- Convert the thesis rules into an implementation-ready catalog.
- Normalize terminology: odd/even threats, zugzwang control, playable square, successor, crossing column, threat combination.
- Produce test fixtures for every named rule:
  `Claimeven`, `Baseinverse`, `Vertical`, `Aftereven`, `Lowinverse`, `Highinverse`, `Baseclaim`, `Before`, `Specialbefore`.

Exit criteria:

- every rule has machine-checkable preconditions and solved-group semantics,
- every combination constraint in chapter 7 is represented explicitly.

### Phase 2: Core Position Model

- Implement a compact board model with legal move generation, win detection, symmetry canonicalization, and group indexing.
- Precompute all 69 winning groups for 7x6.
- Precompute square metadata:
  row parity, successor, predecessor, column height index, group membership.

Exit criteria:

- board operations are deterministic and cheap,
- all legal positions can be evaluated for direct tactical facts in constant or near-constant time.

### Phase 3: Tactical Fact Layer

- Detect immediate wins, forced replies, odd threats, even threats, and threat combinations.
- Determine side-to-move-relative zugzwang controller as required by the thesis evaluation model.
- Support restricted-board evaluation masks for white-side analyses that exclude one or two columns.

Exit criteria:

- the engine can enumerate "real problems" for the defending side:
  opponent groups still completable under current occupancy and mask.

### Phase 4: Rule Instance Generation

- Enumerate all applicable instances of each of the nine rules.
- For each instance, compute:
  covered squares, covered columns, dependency on zugzwang, solved groups, and any side effects.
- Discard rule instances that solve no live problem.

Exit criteria:

- the engine can turn a position into a finite solution set exactly as described for VICTOR.

### Phase 5: Rule Compatibility Graph

- Build the incompatibility graph between rule instances.
- Encode the thesis combination rules:
  disjointness, no-claimeven-below-inverse, column-wise disjoint-or-equal, inverse-column compatibility.
- Validate pairwise compatibility against curated examples from the thesis appendices.

Exit criteria:

- incompatible rules are rejected for the right reason,
- compatibility behavior matches chapter 7 examples.

### Phase 6: Cover Solver

- Solve the "choose a compatible set of solutions covering all problems" task using backtracking with fail-first ordering.
- Prioritize the least-covered problem first, matching the thesis algorithm.
- Return both verdict and explanation:
  chosen rules, covered threats, unresolved residues if any.

Exit criteria:

- resolved positions produce a proof object,
- unresolved positions fail fast with useful diagnostics.

### Phase 7: Move Selection Policy

- Rank legal moves by proof quality:
  immediate win,
  move leading to proven win,
  move preserving proven draw,
  move preserving unresolved-but-promising tactical structure.
- Prefer center and symmetry-preserving moves only as tie-breakers after proof status.
- Expose explanation output:
  "play `d1` because it creates a winning odd-threat structure" is acceptable; opaque scores are not.

Exit criteria:

- the engine can choose moves from rule proofs without generic search as its default behavior.

### Phase 8: Perfect-Play Closure

- Add an offline certification layer for the residual positions the rule engine cannot settle.
- Store only certified positions and best moves in a compact book.
- Use that book during play before invoking any verifier.
- Keep online fallback verification narrow:
  threat-space or conspiracy-style tactical checking only for unresolved states, never full brute-force as the standard loop.

Exit criteria:

- the combined system plays perfectly on 7x6,
- runtime move choice is near-instant for book and proof-covered positions.

## Architecture Workstreams

- `model`: board, moves, symmetry, winning groups.
- `facts`: playable cells, threats, parity, zugzwang control.
- `rules`: nine rule modules plus solved-group generation.
- `solver`: compatibility graph and covering search.
- `policy`: move ranking and explanation assembly.
- `book`: certified lookup store and loaders.
- `cli`: play, analyze, explain, benchmark.
- `tests`: unit, theorem-fixture, regression, certification.

## Verification Strategy

- golden tests from thesis diagrams and appendix rule sets,
- property tests for move legality and symmetry canonicalization,
- regression tests for all known 7x6 initial replies after `1. d1`,
- proof validation tests ensuring every claimed solved group is actually blocked by the selected rule set,
- certification tests that compare book positions against the rule engine and verifier.

## Risks

- The phrase "heuristics" is dangerous here: informal heuristics are not enough for perfect play. The rules must be encoded as proofs, not preferences.
- Zugzwang-dependent interactions are easy to misimplement; parity mistakes will silently corrupt correctness.
- `Before`, `Specialbefore`, `Highinverse`, and threat-combination logic are the most error-prone modules.
- A purely online rule engine is unlikely to prove every critical 7x6 position; offline certification is required for the last mile.

## v1 Success Criteria

- The engine proves or retrieves the correct result for all standard opening branches.
- It never chooses a move contradicted by certified perfect-play data.
- Every non-book move comes with an explanation referencing specific rules or threat structures.
- No default evaluation path depends on brute-force minimax over the whole game tree.
