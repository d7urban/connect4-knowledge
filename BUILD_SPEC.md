# Connect 4 Knowledge-First Perfect-Play Build Spec

## 1. Product Definition

Implement a Connect 4 engine modeled on the thesis "A Knowledge-based Approach of Connect-Four" as a knowledge-first theorem prover for game positions.

Primary behavior:

- derive exact conclusions from formal strategic rules,
- compose compatible rule instances into a proof that all opponent winning groups are neutralized,
- use certified position data only for rule-engine gaps,
- surface human-readable reasons for each move.

Non-goal:

- a conventional brute-force minimax engine with a heuristic evaluation function.

## 2. Supported Board

Target board:

- width: 7
- height: 6
- winning length: 4

Representation must still be parameterized by:

- `ROWS`
- `COLUMNS`
- `GROUPS`

because the thesis architecture generalizes many computations to other even-height boards.

## 3. Core Concepts

### 3.1 Squares

Each square must carry:

- coordinate id,
- column index,
- row index,
- odd/even row parity,
- directly playable status,
- successor square in same column,
- predecessor square in same column,
- member-of-winning-groups list.

### 3.2 Groups

A group is one length-4 horizontal, vertical, or diagonal line. For 7x6, there are 69 groups.

Each group record must include:

- four square ids,
- orientation,
- mask/bitset encoding,
- current occupancy summary:
  white count, black count, empty count,
- live-for-white flag,
- live-for-black flag.

### 3.3 Position

A position object must include:

- side to move,
- occupancy for both players,
- column heights,
- legal move mask,
- winner or terminal status,
- canonical symmetry form,
- optional evaluation mask for restricted-board analyses.

### 3.4 Problem

A problem is an opponent-completable winning group that must be refuted by the defending side's rule set.

Problem record:

- `group_id`
- required squares still open to opponent
- whether the problem is direct, odd-threat-related, even-threat-related, or part of a threat combination

### 3.5 Solution

A solution is one instantiated rule application that solves at least one problem.

Required fields:

- `solution_id`
- `rule_kind`
- participating squares
- participating columns
- zugzwang dependence flag
- solved group bitset
- optional side-effect metadata
- explanation payload

## 4. Rule Inventory

The engine must implement exactly these nine rule families from the thesis:

1. `Claimeven`
2. `Baseinverse`
3. `Vertical`
4. `Aftereven`
5. `Lowinverse`
6. `Highinverse`
7. `Baseclaim`
8. `Before`
9. `Specialbefore`

Each rule module must provide:

- instance enumeration,
- precondition check,
- solved-group derivation,
- involved squares,
- involved columns,
- compatibility signature,
- explanation string builder.

## 5. Rule Semantics Requirements

### 5.1 Claimeven

Represents control of an even square by conceding or mirroring access to the odd square below it in the same column.

Must encode:

- two vertically adjacent empty squares,
- lower square odd, upper square even,
- zugzwang dependence,
- solves groups containing the upper square under the rule conditions.

### 5.2 Baseinverse

Represents control over one of two directly playable squares in distinct columns.

Must encode:

- both squares directly playable,
- no dependency on owning both,
- solves groups containing both squares.

### 5.3 Vertical

Represents the guarantee that if the opponent takes the lower square, the solver takes the square above it.

Must encode:

- two adjacent empty squares in one column,
- upper square typically odd in normal use,
- no zugzwang dependence,
- solves groups containing both squares.

### 5.4 Aftereven

Represents a compound rule built from one or more `Claimeven` instances plus an aftereven group.

Must encode:

- aftereven group with empty squares spread across columns,
- component claimevens,
- derived blocking effect on groups above aftereven columns,
- solved groups from both side effect and component clauses.

### 5.5 Lowinverse

Represents linked inverse behavior across two columns, each with an even-over-odd pair.

Must encode:

- two columns,
- two empty squares per column,
- vertical side effects after one lower square is triggered,
- guarantee of at least one target odd square.

### 5.6 Highinverse

Represents the higher analogue of `Lowinverse` with three squares per column and stronger joint claims.

Must encode:

- two columns,
- three empty squares per column,
- main inverse goals plus vertical side effects,
- special handling of upper two squares.

### 5.7 Baseclaim

Represents a hybrid of two `Baseinverse` patterns and one `Claimeven`.

Must encode:

- two playable squares,
- one playable/non-playable pair,
- contingent resolution depending on opponent move choice,
- mixed solved-group reasoning.

### 5.8 Before

Represents a future-completion argument using a before group plus supporting `Claimeven` and `Vertical` parts.

Must encode:

- before group with no opponent stones,
- all empty before-group squares not in top row,
- successor-square logic,
- side effects from supporting parts.

### 5.9 Specialbefore

Represents the special case of `Before` where one empty square is directly playable and an extra playable square is integrated.

Must encode:

- specialbefore group,
- one directly playable empty square in group,
- extra playable square,
- vertical/claimeven support,
- contingent baseinverse-like branch behavior.

## 6. Compatibility Rules

Solutions form an incompatibility graph. Two solutions are incompatible if they violate the chapter 7 combination rules.

Encode these constraints exactly:

1. allowed if square sets are disjoint
2. allowed if no `Claimeven` is below the inverse
3. allowed if square sets are column-wise disjoint or equal
4. allowed if square sets are disjoint and inverse-used columns are disjoint or equal

Additional notes:

- when two constraints are listed for a pair, both must hold,
- `Specialbefore` special squares do not count as equal to another rule's squares unless the shared structure is exactly a shared component as described in the thesis,
- compatibility must be represented as data, not scattered conditionals.

## 7. Evaluation Pipeline

For any given position:

1. Determine evaluator perspective.
   White to move -> evaluate with Black controlling zugzwang.
   Black to move -> evaluate with White controlling zugzwang, possibly on a masked sub-board if odd-threat or threat-combination logic requires it.
2. Enumerate opponent-live groups.
3. Convert those groups into problems.
4. Enumerate all candidate rule instances.
5. Drop any instance that solves no current problem.
6. Build the incompatibility graph.
7. Search for an independent set of solutions that covers all problems.
8. If found, emit exact verdict and proof.
9. If not found, mark unresolved and hand off to certification layer.

## 8. Cover Solver

Implement the cover solver as recursive backtracking over:

- remaining problems,
- remaining allowed solutions.

Required heuristics:

- choose the problem with the fewest currently legal solving solutions,
- branch over those candidate solutions,
- prune immediately when a remaining problem has zero legal solutions.

Required outputs:

- `SolvedWin`
- `SolvedDraw`
- `Unresolved`

Optional output for future extension:

- `SolvedLossForOpponentByAftereven`

## 9. Move Policy

Move selection order:

1. immediate winning move
2. move to child position with certified win
3. move to child position with rule proof of win
4. move to child position with certified draw when no win exists
5. move to child position with rule proof of draw when no certified result exists
6. unresolved move only if verifier confirms it is equivalent to best certified result

Tie-breakers:

- center preference,
- symmetry reduction,
- shortest proof,
- smallest verifier cost.

The engine must never choose a move solely because it has a high scalar heuristic score.

## 10. Certified Knowledge Layer

To achieve perfect play on 7x6 without making brute-force search the online policy, implement a certified knowledge layer.

Requirements:

- offline generation only,
- positions stored in canonical form,
- exact result plus best move set,
- explanation tag:
  `book`, `rule-proof`, or `verifier`.

Preferred initial contents:

- all opening branches needed to certify perfect first-player play,
- high-friction tactical positions the rule engine routinely leaves unresolved.

## 11. Verifier Layer

The verifier exists only to close proof gaps.

Allowed verifier behavior:

- threat-space search,
- conspiracy-number-inspired tactical search,
- small exact search around unresolved tactical fronts,
- transposition table and symmetry reduction.

Disallowed verifier behavior:

- unrestricted full-tree brute-force as the default play loop,
- opaque heuristic pruning that breaks correctness guarantees.

Verifier outputs must be certifiable:

- exact result,
- principal variation or proof witness,
- nodes visited and reason for invocation.

## 12. Explanation Interface

Every analyzed move must be able to report:

- verdict:
  win, draw, unresolved
- basis:
  immediate tactic, rule proof, book, verifier
- named rules used
- major threatened groups or neutralized groups

CLI examples:

- `play`
- `analyze <position>`
- `explain <move>`
- `dump-proof <position>`

## 13. Testing Requirements

### 13.1 Unit Tests

- board legality,
- move application,
- win detection,
- playable-square detection,
- group enumeration,
- symmetry canonicalization.

### 13.2 Rule Tests

For each of the nine rules:

- positive fixture from thesis-style diagrams,
- negative fixture,
- solved-group verification,
- compatibility spot checks.

### 13.3 Integration Tests

- full evaluation of appendix-B style positions,
- opening tests after `1. d1` with each black reply,
- positions where the rule engine should return unresolved,
- positions where the book must override expensive verification.

### 13.4 Soundness Tests

- every claimed solved group must be independently checked against the rule definition,
- every accepted solution set must be pairwise compatible,
- no proof may depend on a square outside the evaluation mask.

## 14. Performance Targets

- legal move generation: effectively constant time
- rule enumeration: low milliseconds for typical midgame positions
- cover solving: low milliseconds for most proof-covered positions
- gameplay response:
  under 50 ms for book/proof positions,
  under 500 ms for verifier-assisted positions in normal play

## 15. Recommended Implementation Shape

Suggested modules:

- `board.rs`
- `groups.rs`
- `facts.rs`
- `rules/mod.rs`
- `rules/claimeven.rs`
- `rules/baseinverse.rs`
- `rules/vertical.rs`
- `rules/aftereven.rs`
- `rules/lowinverse.rs`
- `rules/highinverse.rs`
- `rules/baseclaim.rs`
- `rules/before.rs`
- `rules/specialbefore.rs`
- `compat.rs`
- `solver.rs`
- `book.rs`
- `verifier.rs`
- `policy.rs`
- `cli.rs`

## 16. Acceptance Criteria

- The implementation reproduces the thesis rule system faithfully enough to explain and solve the known non-center-opening draw schemes for Black.
- The implementation can certify that `1. d1` is the only winning first move on 7x6 when combined with the certified knowledge layer.
- During ordinary play, the engine relies on rule proofs and certified lookups first, not brute-force search.
- Every reported exact result is auditable through a proof object, a certified book entry, or a verifier trace.
