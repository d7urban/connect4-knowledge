use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::board::{Color, Column, Position};
use crate::book::{BookEntry, EntrySource, verifier_entry_for_position};
use crate::bookdb::{
    CertifyChildRecord, CertifyChildStatus, CertifyNodeState, RedbBookStore, default_book_db_path,
};
use crate::facts::{EvaluationMask, analyze_position};
use crate::rules::generate_all;
use crate::solver::Verdict;
use crate::verifier::{ExactValue, SearchConfig, search_best_move, verify_with_config, verify_with_outcome};

pub const DEFAULT_FRONTIER_PATH: &str = "book_frontier.json";
const VERIFIER_MAX_EMPTIES: usize = 18;
const OPENING_BAND_DEPTH: usize = 6;
const TACTICAL_THREAT_THRESHOLD: usize = 4;
const NEAR_VERIFIER_EMPTIES: usize = 22;
const RESEED_PENDING_THRESHOLD: usize = 24;
const RESEED_TARGET_NEW_ENTRIES: usize = 256;
const BASE_RESEED_DEPTH: usize = 8;
const TARGET_SEED_PLY_MIN: usize = 20;
const TARGET_SEED_PLY_MAX: usize = 30;
const TARGET_SEED_COUNT: usize = 24;
const TARGET_DESCENT_STEPS: usize = 12;
const TARGET_BUCKET_WIDTH: usize = 2;
const TARGET_BUCKETS_PER_STAGE: usize = 3;
const TARGET_VARIANTS_PER_SEED: usize = 3;
const DEFAULT_CHECKPOINT_EVERY: usize = 1_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FrontierEntry {
    moves: Vec<Column>,
    depth: usize,
    empties: usize,
    times_seen: usize,
    threat_score: usize,
    problems: usize,
    solutions: usize,
    rule_unresolved: bool,
    verifier_ready: bool,
    priority: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FrontierState {
    pending: Vec<FrontierEntry>,
    seen: HashSet<String>,
    reseed_stage: usize,
}

impl Default for FrontierState {
    fn default() -> Self {
        let mut seen = HashSet::new();
        seen.insert(position_key(&Position::new()));
        let root = frontier_entry(Vec::new(), &Position::new());
        Self {
            pending: vec![root],
            seen,
            reseed_stage: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExpandReport {
    pub processed: usize,
    pub enqueued: usize,
    pub verifier_solved: usize,
    pub book_inserted: usize,
    pub book_updated: usize,
    pub verifier_attempted: usize,
    pub verifier_skipped: usize,
    pub verifier_cached: usize,
    pub expansion_pruned: usize,
    pub frontier_reseeded: usize,
    pub checkpoints_written: usize,
    pub pending_remaining: usize,
    pub book_entries: usize,
}

#[derive(Debug, Clone)]
pub struct CertifyReport {
    pub certified: bool,
    pub exact_value: Option<ExactValue>,
    pub best_move: Option<Column>,
    pub inserted: bool,
    pub updated: bool,
    pub nodes_visited: usize,
    pub book_hits: usize,
    pub certify_cache_hits: usize,
    pub certify_tt_hits: usize,
    pub certify_frontier_resumed: usize,
    pub certify_frontier_solved: usize,
    pub certify_frontier_requeued: usize,
    pub certify_frontier_remaining: usize,
    pub certify_frontier_min_empties: Option<usize>,
    pub certify_frontier_max_empties: Option<usize>,
    pub verifier_hits: usize,
    pub root_children: Vec<RootChildReport>,
    pub explanation: String,
}

#[derive(Debug, Clone)]
pub struct RootChildReport {
    pub column: Column,
    pub exact_value: Option<ExactValue>,
    pub known_children: usize,
    pub attempted_unresolved_children: usize,
    pub total_children: usize,
    pub frontier_count: usize,
    pub frontier_min_empties: Option<usize>,
}

#[derive(Debug, Clone)]
struct RootGrandchildTarget {
    root_child: Column,
    grandchild: Option<Column>,
    attempted_unresolved: bool,
    frontier_count: usize,
    frontier_min_empties: Option<usize>,
    known_children: usize,
}

#[derive(Debug, Clone)]
pub struct RootChildStateDump {
    pub column: Column,
    pub exact_value: Option<ExactValue>,
    pub known_children: usize,
    pub attempted_unresolved_children: usize,
    pub total_children: usize,
    pub frontier_count: usize,
    pub frontier_min_empties: Option<usize>,
    pub child_statuses: Vec<RootGrandchildStateDump>,
}

#[derive(Debug, Clone)]
pub struct RootGrandchildStateDump {
    pub column: Column,
    pub status: CertifyChildStatus,
    pub frontier_count: usize,
    pub frontier_min_empties: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct CertifyRootStateReport {
    pub position_label: String,
    pub frontier_remaining: usize,
    pub root_children: Vec<RootChildStateDump>,
}

struct ExpandRunOptions {
    reset_frontier: bool,
    checkpoint_every: usize,
    initial_state: FrontierState,
    allow_reseed: bool,
    branch_seed_moves: Option<Vec<Column>>,
}

#[derive(Default)]
struct CertifyCtx {
    max_nodes: usize,
    nodes: usize,
    book_hits: usize,
    certify_cache_hits: usize,
    certify_tt_hits: usize,
    certify_frontier_resumed: usize,
    certify_frontier_solved: usize,
    certify_frontier_requeued: usize,
    certify_frontier_min_empties: Option<usize>,
    certify_frontier_max_empties: Option<usize>,
    verifier_hits: usize,
    cache: std::collections::HashMap<String, Option<ExactValue>>,
    progress_enabled: bool,
    next_progress_percent: usize,
    started_at: Option<Instant>,
}

pub fn expand_book(batch_limit: usize) -> Result<ExpandReport, String> {
    expand_book_with_options(batch_limit, false, DEFAULT_CHECKPOINT_EVERY)
}

pub fn certify_position_with_options(
    moves: &[Column],
    max_nodes: usize,
) -> Result<CertifyReport, String> {
    let position = position_from_moves(moves)
        .map_err(|err| format!("invalid position moves: {err}"))?;
    let store = RedbBookStore::open_or_create(default_book_db_path()).map_err(|err| err.to_string())?;
    cleanup_legacy_certify_frontier(&store, &position, position.side_to_move)?;

    if let Some(entry) = store.get(&position).map_err(|err| err.to_string())?
        && let Some(exact_value) = entry.exact_value
    {
        return Ok(CertifyReport {
            certified: true,
            exact_value: Some(exact_value),
            best_move: entry.best_moves.first().copied(),
            inserted: false,
            updated: false,
            nodes_visited: 0,
            book_hits: 1,
            certify_cache_hits: 0,
            certify_tt_hits: 0,
            certify_frontier_resumed: 0,
            certify_frontier_solved: 0,
            certify_frontier_requeued: 0,
            certify_frontier_remaining: 0,
            certify_frontier_min_empties: None,
            certify_frontier_max_empties: None,
            verifier_hits: 0,
            root_children: Vec::new(),
            explanation: "position already has an exact book entry".to_string(),
        });
    }

    let mut ctx = CertifyCtx {
        max_nodes,
        progress_enabled: max_nodes >= 10_000,
        next_progress_percent: 5,
        started_at: Some(Instant::now()),
        ..Default::default()
    };
    start_certify_progress(&ctx);
    let mut witness = Vec::new();
    let mut pending_book_writes = Vec::new();
    let blocker_budget = max_nodes.saturating_mul(8) / 10;
    if blocker_budget > 0 {
        process_root_child_blockers(
            &store,
            &position,
            position.side_to_move,
            &mut ctx,
            &mut pending_book_writes,
            blocker_budget,
        )?;
    }
    let frontier_budget = max_nodes / 10;
    if frontier_budget > 0 {
        process_persistent_certify_frontier(
            &store,
            &position,
            position.side_to_move,
            &mut ctx,
            &mut pending_book_writes,
            frontier_budget,
        )?;
    }
    let Some(target_value) = certify_exact_value(
        &store,
        &position,
        &position,
        position.side_to_move,
        None,
        None,
        &mut ctx,
        &mut witness,
        &mut pending_book_writes,
    )?
    else {
        flush_book_writes(&store, &mut pending_book_writes)?;
        finish_certify_progress(&ctx);
        let root_children = analyze_root_children(&store, &position, position.side_to_move)?;
        return Ok(CertifyReport {
            certified: false,
            exact_value: None,
            best_move: None,
            inserted: false,
            updated: false,
            nodes_visited: ctx.nodes,
            book_hits: ctx.book_hits,
            certify_cache_hits: ctx.certify_cache_hits,
            certify_tt_hits: ctx.certify_tt_hits,
            certify_frontier_resumed: ctx.certify_frontier_resumed,
            certify_frontier_solved: ctx.certify_frontier_solved,
            certify_frontier_requeued: ctx.certify_frontier_requeued,
            certify_frontier_remaining: store
                .count_certify_frontier(&position, position.side_to_move)
                .map_err(|err| err.to_string())?,
            certify_frontier_min_empties: ctx.certify_frontier_min_empties,
            certify_frontier_max_empties: ctx.certify_frontier_max_empties,
            verifier_hits: ctx.verifier_hits,
            root_children,
            explanation: "position could not be certified within the current exact-search budget".to_string(),
        });
    };

    let root_exact_value = target_value_to_side_value(&position, position.side_to_move, target_value);
    let best_moves = witness.first().copied().map(|column| vec![column]).unwrap_or_default();
    let existed = store.get(&position).map_err(|err| err.to_string())?.is_some();
    pending_book_writes.push(verifier_entry_for_position(
        &position,
        root_exact_value,
        best_moves.clone(),
    ));
    flush_book_writes(&store, &mut pending_book_writes)?;
    finish_certify_progress(&ctx);
    let root_children = analyze_root_children(&store, &position, position.side_to_move)?;

    Ok(CertifyReport {
        certified: true,
        exact_value: Some(root_exact_value),
        best_move: best_moves.first().copied(),
        inserted: !existed,
        updated: existed,
        nodes_visited: ctx.nodes,
        book_hits: ctx.book_hits,
        certify_cache_hits: ctx.certify_cache_hits,
        certify_tt_hits: ctx.certify_tt_hits,
        certify_frontier_resumed: ctx.certify_frontier_resumed,
        certify_frontier_solved: ctx.certify_frontier_solved,
        certify_frontier_requeued: ctx.certify_frontier_requeued,
        certify_frontier_remaining: store
            .count_certify_frontier(&position, position.side_to_move)
            .map_err(|err| err.to_string())?,
        certify_frontier_min_empties: ctx.certify_frontier_min_empties,
        certify_frontier_max_empties: ctx.certify_frontier_max_empties,
        verifier_hits: ctx.verifier_hits,
        root_children,
        explanation: "position certified by exact search using book-oracle leaves and verifier closure".to_string(),
    })
}

pub fn inspect_certify_root_state(moves: &[Column]) -> Result<CertifyRootStateReport, String> {
    let position = position_from_moves(moves)
        .map_err(|err| format!("invalid position moves: {err}"))?;
    let store = RedbBookStore::open_or_create(default_book_db_path()).map_err(|err| err.to_string())?;
    cleanup_legacy_certify_frontier(&store, &position, position.side_to_move)?;
    let root_children = inspect_certify_root_state_from_store(&store, &position, position.side_to_move)?;

    Ok(CertifyRootStateReport {
        position_label: moves
            .iter()
            .map(|column| (b'a' + column.0 as u8) as char)
            .collect::<String>(),
        frontier_remaining: store
            .count_certify_frontier(&position, position.side_to_move)
            .map_err(|err| err.to_string())?,
        root_children,
    })
}

fn cleanup_legacy_certify_frontier(
    store: &RedbBookStore,
    root: &Position,
    target: Color,
) -> Result<(), String> {
    let entries = store
        .list_certify_frontier(root, target, usize::MAX)
        .map_err(|err| err.to_string())?;
    for entry in entries {
        if entry.root_child.is_some()
            && entry.root_grandchild.is_none()
            && let Some(position) = position_from_canonical_state(&entry.canonical_key, entry.side_to_move)
        {
            store
                .remove_certify_frontier(root, target, &position)
                .map_err(|err| err.to_string())?;
        }
    }
    Ok(())
}

fn inspect_certify_root_state_from_store(
    store: &RedbBookStore,
    root: &Position,
    target: Color,
) -> Result<Vec<RootChildStateDump>, String> {
    let frontier_entries = store
        .list_certify_frontier(root, target, usize::MAX)
        .map_err(|err| err.to_string())?;
    let mut frontier_by_root_child = BTreeMap::<usize, (usize, Option<usize>)>::new();
    let mut frontier_by_pair = BTreeMap::<(usize, usize), (usize, Option<usize>)>::new();
    for entry in frontier_entries {
        if let Some(root_child) = entry.root_child {
            let stats = frontier_by_root_child.entry(root_child.0).or_insert((0, None));
            stats.0 += 1;
            stats.1 = Some(stats.1.map_or(entry.empties, |current: usize| current.min(entry.empties)));
            if let Some(root_grandchild) = entry.root_grandchild {
                let pair_stats = frontier_by_pair
                    .entry((root_child.0, root_grandchild.0))
                    .or_insert((0, None));
                pair_stats.0 += 1;
                pair_stats.1 = Some(
                    pair_stats
                        .1
                        .map_or(entry.empties, |current: usize| current.min(entry.empties)),
                );
            }
        }
    }

    let mut root_children = Vec::new();
    for column in root.legal_moves() {
        let mut child = root.clone();
        child.apply_move(column)
            .map_err(|err| format!("cannot inspect root child: {err}"))?;
        let exact_value = persisted_exact_value(store, &child, target)?;
        let node_state = store
            .get_certify_node_state(&child, target)
            .map_err(|err| err.to_string())?;
        let child_statuses = node_state
            .as_ref()
            .map(|state| {
                state
                    .children
                    .iter()
                    .map(|child| {
                        let (frontier_count, frontier_min_empties) = frontier_by_pair
                            .get(&(column.0, child.column.0))
                            .copied()
                            .unwrap_or((0, None));
                        RootGrandchildStateDump {
                            column: child.column,
                            status: child.status,
                            frontier_count,
                            frontier_min_empties,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let (known_children, attempted_unresolved_children, total_children) = node_state
            .as_ref()
            .map(|state| {
                (
                    state
                        .children
                        .iter()
                        .filter(|child| {
                            matches!(
                                child.status,
                                CertifyChildStatus::ExactWin
                                    | CertifyChildStatus::ExactDraw
                                    | CertifyChildStatus::ExactLoss
                            )
                        })
                        .count(),
                    state
                        .children
                        .iter()
                        .filter(|child| child.status == CertifyChildStatus::AttemptedUnresolved)
                        .count(),
                    state.children.len(),
                )
            })
            .unwrap_or_else(|| (0, 0, child.legal_moves().len()));
        let (frontier_count, frontier_min_empties) = frontier_by_root_child
            .get(&column.0)
            .copied()
            .unwrap_or((0, None));
        root_children.push(RootChildStateDump {
            column,
            exact_value,
            known_children,
            attempted_unresolved_children,
            total_children,
            frontier_count,
            frontier_min_empties,
            child_statuses,
        });
    }

    Ok(root_children)
}

pub fn expand_book_with_options(
    batch_limit: usize,
    reset_frontier: bool,
    checkpoint_every: usize,
) -> Result<ExpandReport, String> {
    expand_book_with_paths(
        default_book_db_path(),
        default_frontier_path(),
        batch_limit,
        reset_frontier,
        checkpoint_every,
    )
}

pub fn expand_branch_with_options(
    seed_moves: &[Column],
    batch_limit: usize,
    reset_frontier: bool,
    checkpoint_every: usize,
) -> Result<(ExpandReport, PathBuf), String> {
    let position = position_from_moves(seed_moves)
        .map_err(|err| format!("invalid branch moves: {err}"))?;
    let frontier_path = branch_frontier_path(seed_moves);
    let initial_state = branch_frontier_state(seed_moves.to_vec(), position);
    let report = expand_book_with_paths_and_state(
        default_book_db_path(),
        frontier_path.clone(),
        batch_limit,
        ExpandRunOptions {
            reset_frontier,
            checkpoint_every,
            initial_state,
            allow_reseed: false,
            branch_seed_moves: Some(seed_moves.to_vec()),
        },
    )?;
    Ok((report, frontier_path))
}

pub fn expand_book_with_paths(
    book_path: PathBuf,
    frontier_path: PathBuf,
    batch_limit: usize,
    reset_frontier: bool,
    checkpoint_every: usize,
) -> Result<ExpandReport, String> {
    expand_book_with_paths_and_state(
        book_path,
        frontier_path,
        batch_limit,
        ExpandRunOptions {
            reset_frontier,
            checkpoint_every,
            initial_state: FrontierState::default(),
            allow_reseed: true,
            branch_seed_moves: None,
        },
    )
}

fn expand_book_with_paths_and_state(
    book_path: PathBuf,
    frontier_path: PathBuf,
    batch_limit: usize,
    options: ExpandRunOptions,
) -> Result<ExpandReport, String> {
    let store = RedbBookStore::open_or_create(&book_path).map_err(|err| err.to_string())?;
    let mut state = if options.reset_frontier {
        options.initial_state
    } else {
        load_frontier_or_default(&frontier_path, options.initial_state).map_err(|err| err.to_string())?
    };

    let mut processed = 0usize;
    let mut enqueued = 0usize;
    let mut verifier_solved = 0usize;
    let mut book_inserted = 0usize;
    let mut book_updated = 0usize;
    let mut verifier_attempted = 0usize;
    let mut verifier_skipped = 0usize;
    let mut verifier_cached = 0usize;
    let mut expansion_pruned = 0usize;
    let mut frontier_reseeded = 0usize;
    let mut checkpoints_written = 0usize;
    let mut pending_book_writes = Vec::<BookEntry>::new();

    while processed < batch_limit {
        if options.allow_reseed && should_reseed(&state) {
            let entries = store.all_entries().map_err(|err| err.to_string())?;
            frontier_reseeded += reseed_opening_frontier(&mut state, &entries);
        }
        if state.pending.is_empty() {
            break;
        }

        sort_pending(&mut state.pending);
        let entry = state.pending.remove(0);
        let position = position_from_moves(&entry.moves)
            .map_err(|err| format!("invalid frontier path: {err}"))?;

        let mut solved = false;
        let mut cached_verifier_hit = false;
        if entry.verifier_ready {
            if verifier_already_cached(&store, &position) {
                verifier_cached += 1;
                solved = true;
                cached_verifier_hit = true;
            } else {
                verifier_attempted += 1;
                if let Some(result) = verify_with_config(
                    &position,
                    position.side_to_move,
                    SearchConfig {
                        max_nodes: 1_000_000,
                        max_empties: VERIFIER_MAX_EMPTIES,
                        iterative_deepening: true,
                    },
                ) {
                    let best_moves = result
                        .witness
                        .as_ref()
                        .and_then(|line| line.first().copied())
                        .map(|column| vec![column])
                        .unwrap_or_default();
                    let inserted_new = store.get(&position).map_err(|err| err.to_string())?.is_none()
                        && !pending_book_writes.iter().any(|entry| {
                            entry.canonical_key == position.board.canonical_key().to_vec()
                                && entry.side_to_move == position.side_to_move
                        });
                    pending_book_writes.push(verifier_entry_for_position(
                        &position,
                        result.verdict,
                        best_moves,
                    ));
                    if inserted_new {
                        book_inserted += 1;
                    } else {
                        book_updated += 1;
                    }
                    verifier_solved += 1;
                    solved = true;
                }
            }
        } else {
            verifier_skipped += 1;
        }

        if cached_verifier_hit {
            expansion_pruned += 1;
            processed += 1;
            continue;
        }

        let expand_all_children = solved || entry.verifier_ready;
        let allow_limited_expansion = entry.depth < OPENING_BAND_DEPTH
            || entry.threat_score >= TACTICAL_THREAT_THRESHOLD
            || entry.empties <= NEAR_VERIFIER_EMPTIES;
        if !expand_all_children && !allow_limited_expansion {
            expansion_pruned += 1;
            processed += 1;
            continue;
        }

        let candidate_moves = if expand_all_children {
            ordered_moves(position.legal_moves())
        } else {
            principal_line_moves(&position)
        };

        if candidate_moves.is_empty() {
            expansion_pruned += 1;
            processed += 1;
            continue;
        }

        for column in candidate_moves {
            let mut child = position.clone();
            child.apply_move(column)
                .map_err(|err| format!("cannot expand child move: {err}"))?;
            let child_path = extend_path(&entry.moves, column);
            let key = position_key(&child);
            if state.seen.insert(key) {
                state.pending.push(frontier_entry(child_path, &child));
                enqueued += 1;
            } else if let Some(existing) = state
                .pending
                .iter_mut()
                .find(|existing| existing.moves == child_path)
            {
                existing.times_seen += 1;
                existing.priority = priority(existing);
            }
        }

        processed += 1;

        if checkpoint_due(processed, options.checkpoint_every, batch_limit) {
            flush_book_writes(&store, &mut pending_book_writes)?;
            save_frontier(&frontier_path, &state)
                .map_err(|err| format!("failed to save frontier checkpoint: {err}"))?;
            checkpoints_written += 1;
        }
    }

    flush_book_writes(&store, &mut pending_book_writes)?;
    if let Some(seed_moves) = options.branch_seed_moves.as_deref()
        && let Some((inserted, updated)) = propagate_branch_root_entries(&store, seed_moves)?
    {
        book_inserted += inserted;
        book_updated += updated;
    }
    save_frontier(&frontier_path, &state)
        .map_err(|err| format!("failed to save frontier: {err}"))?;
    if !checkpoint_due(processed, options.checkpoint_every, batch_limit) || processed == 0 {
        checkpoints_written += 1;
    }

    Ok(ExpandReport {
        processed,
        enqueued,
        verifier_solved,
        book_inserted,
        book_updated,
        verifier_attempted,
        verifier_skipped,
        verifier_cached,
        expansion_pruned,
        frontier_reseeded,
        checkpoints_written,
        pending_remaining: state.pending.len(),
        book_entries: store.len().map_err(|err| err.to_string())?,
    })
}

pub fn default_frontier_path() -> PathBuf {
    PathBuf::from(DEFAULT_FRONTIER_PATH)
}

pub fn default_checkpoint_every() -> usize {
    DEFAULT_CHECKPOINT_EVERY
}

fn position_key(position: &Position) -> String {
    let side = match position.side_to_move {
        Color::White => 'w',
        Color::Black => 'b',
    };
    let board = position
        .board
        .canonical_key()
        .iter()
        .map(|value| char::from(b'0' + *value))
        .collect::<String>();
    format!("{side}:{board}")
}

fn extend_path(path: &[Column], column: Column) -> Vec<Column> {
    let mut next = path.to_vec();
    next.push(column);
    next
}

fn frontier_entry(moves: Vec<Column>, position: &Position) -> FrontierEntry {
    let facts = analyze_position(position, EvaluationMask::default());
    let solutions = generate_all(position, &facts);
    let empties = 42usize.saturating_sub(position.move_count);
    let threat_score = immediate_wins(position, position.side_to_move)
        + immediate_wins(position, position.side_to_move.opponent())
        + facts.problems.len().min(12);
    let mut entry = FrontierEntry {
        depth: moves.len(),
        moves,
        empties,
        times_seen: 1,
        threat_score,
        problems: facts.problems.len(),
        solutions: solutions.len(),
        rule_unresolved: facts.problems.len() > 18 || solutions.len() > 160,
        verifier_ready: empties <= 18,
        priority: 0,
    };
    entry.priority = priority(&entry);
    entry
}

fn priority(entry: &FrontierEntry) -> i64 {
    let depth_pressure = (42usize.saturating_sub(entry.empties)) as i64 * 220;
    let verifier_distance = if entry.verifier_ready {
        5_000
    } else {
        (42usize.saturating_sub(entry.empties.saturating_sub(VERIFIER_MAX_EMPTIES))) as i64 * 90
    };
    let repeated_seen = entry.times_seen as i64 * 35;
    let threat = entry.threat_score as i64 * 55;
    let rule_gap = if entry.rule_unresolved { 500 } else { 0 };
    let verifier_ready = if entry.verifier_ready {
        1_500 + (VERIFIER_MAX_EMPTIES.saturating_sub(entry.empties)) as i64 * 75
    } else {
        0
    };
    let low_branch_bonus = (200usize.saturating_sub(entry.solutions.min(200))) as i64;
    let opening_penalty = entry.depth.min(10) as i64 * -15;

    depth_pressure + verifier_distance + repeated_seen + threat + rule_gap + verifier_ready + low_branch_bonus + opening_penalty
}

fn sort_pending(pending: &mut [FrontierEntry]) {
    pending.sort_by(|left, right| {
        right
            .priority
            .cmp(&left.priority)
            .then(left.depth.cmp(&right.depth))
            .then(left.empties.cmp(&right.empties))
    });
}

fn immediate_wins(position: &Position, mover: Color) -> usize {
    if mover != position.side_to_move {
        let mut swapped = position.clone();
        swapped.side_to_move = mover;
        return immediate_wins(&swapped, mover);
    }

    position
        .legal_moves()
        .into_iter()
        .filter(|&column| {
            let mut child = position.clone();
            child.apply_move(column).is_ok() && child.winner == Some(mover)
        })
        .count()
}

fn ordered_moves(mut moves: Vec<Column>) -> Vec<Column> {
    moves.sort_by_key(|column| (3_i32 - column.0 as i32).abs());
    moves
}

fn principal_line_moves(position: &Position) -> Vec<Column> {
    let mut ordered = ordered_moves(position.legal_moves());
    if ordered.is_empty() {
        return ordered;
    }

    if let Some(search) = search_best_move(position, position.side_to_move, 6, 120_000)
        && let Some(best_idx) = ordered.iter().position(|&column| column == search.best_move)
    {
        let best = ordered.remove(best_idx);
        ordered.insert(0, best);
    }

    let empties = 42usize.saturating_sub(position.move_count);
    let cap = if empties <= NEAR_VERIFIER_EMPTIES && !position.move_count.is_multiple_of(2) {
        2
    } else {
        1
    };
    ordered.into_iter().take(cap).collect()
}

fn position_from_moves(moves: &[Column]) -> Result<Position, crate::board::MoveError> {
    let mut position = Position::new();
    for &column in moves {
        position.apply_move(column)?;
    }
    Ok(position)
}

fn load_frontier_or_default(path: &Path, default_state: FrontierState) -> io::Result<FrontierState> {
    if !path.exists() {
        return Ok(default_state);
    }

    let contents = fs::read_to_string(path)?;
    let mut state: FrontierState =
        serde_json::from_str(&contents).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    sort_pending(&mut state.pending);
    Ok(state)
}

fn branch_frontier_state(seed_moves: Vec<Column>, position: Position) -> FrontierState {
    let mut seen = HashSet::new();
    seen.insert(position_key(&position));
    FrontierState {
        pending: vec![frontier_entry(seed_moves, &position)],
        seen,
        reseed_stage: 0,
    }
}

fn branch_frontier_path(seed_moves: &[Column]) -> PathBuf {
    let suffix = if seed_moves.is_empty() {
        "root".to_string()
    } else {
        seed_moves
            .iter()
            .map(|column| char::from(b'a' + column.0 as u8))
            .collect::<String>()
    };
    PathBuf::from(format!("book_frontier_branch_{suffix}.json"))
}

fn save_frontier(path: &Path, state: &FrontierState) -> io::Result<()> {
    let contents = serde_json::to_string_pretty(state)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    fs::write(path, contents)
}

fn verifier_already_cached(store: &RedbBookStore, position: &Position) -> bool {
    matches!(
        store.get(position).ok().flatten(),
        Some(entry)
            if matches!(entry.source, EntrySource::Book)
                || matches!(entry.source, EntrySource::Verifier) && entry.exact_value.is_some()
    )
}

fn should_reseed(state: &FrontierState) -> bool {
    state.pending.len() <= RESEED_PENDING_THRESHOLD
        && !state.pending.iter().any(|entry| entry.verifier_ready)
}

fn reseed_opening_frontier(state: &mut FrontierState, entries: &[BookEntry]) -> usize {
    let seeds = targeted_seed_positions(entries, state.reseed_stage);
    if !seeds.is_empty() {
        let mut inserted = 0usize;
        for (position, path) in seeds {
            inserted += descend_from_seed(state, entries, &position, &path, TARGET_DESCENT_STEPS);
            if inserted >= RESEED_TARGET_NEW_ENTRIES {
                break;
            }
        }
        if inserted > 0 {
            state.reseed_stage += 1;
            return inserted;
        }
    }

    let depth_limit = BASE_RESEED_DEPTH + state.reseed_stage.min(6);
    let mut stack = vec![(Position::new(), Vec::new())];
    let mut inserted = 0usize;

    while let Some((position, path)) = stack.pop() {
        if path.len() >= depth_limit || inserted >= RESEED_TARGET_NEW_ENTRIES {
            continue;
        }

        let mut next_paths = Vec::new();
        for column in reseed_moves(&position, entries, state.reseed_stage) {
            let mut child = position.clone();
            if child.apply_move(column).is_err() {
                continue;
            }
            let child_path = extend_path(&path, column);
            let key = position_key(&child);
            if state.seen.insert(key) {
                state.pending.push(frontier_entry(child_path.clone(), &child));
                inserted += 1;
            }
            next_paths.push((child, child_path));
            if inserted >= RESEED_TARGET_NEW_ENTRIES {
                break;
            }
        }

        while let Some(next) = next_paths.pop() {
            stack.push(next);
        }
    }

    if inserted > 0 {
        state.reseed_stage += 1;
    }
    inserted
}

fn targeted_seed_positions(entries: &[BookEntry], stage: usize) -> Vec<(Position, Vec<Column>)> {
    let scarce_buckets = scarce_target_buckets(entries);
    let mut seeds = entries
        .iter()
        .filter_map(|entry| {
            let ply = entry.canonical_key.iter().filter(|&&cell| cell != 0).count();
            if !(TARGET_SEED_PLY_MIN..=TARGET_SEED_PLY_MAX).contains(&ply) {
                return None;
            }
            let bucket = ply_bucket_start(ply);
            if !scarce_buckets.contains_key(&bucket) {
                return None;
            }
            let position = position_from_canonical_entry(entry)?;
            Some((seed_priority(entry, ply, stage, *scarce_buckets.get(&bucket).unwrap_or(&0)), position))
        })
        .collect::<Vec<_>>();

    seeds.sort_by(|left, right| right.0.cmp(&left.0));
    seeds
        .into_iter()
        .take(TARGET_SEED_COUNT)
        .flat_map(|(_, position)| {
            principal_seed_paths(&position, stage)
                .into_iter()
                .map(move |path| (position.clone(), path))
        })
        .take(RESEED_TARGET_NEW_ENTRIES.min(TARGET_SEED_COUNT * TARGET_VARIANTS_PER_SEED))
        .collect()
}

fn scarce_target_buckets(entries: &[BookEntry]) -> BTreeMap<usize, usize> {
    let mut counts = BTreeMap::<usize, usize>::new();
    for entry in entries {
        let ply = entry.canonical_key.iter().filter(|&&cell| cell != 0).count();
        if (TARGET_SEED_PLY_MIN..=TARGET_SEED_PLY_MAX).contains(&ply) {
            *counts.entry(ply_bucket_start(ply)).or_default() += 1;
        }
    }

    let mut ranked = counts.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| left.1.cmp(&right.1).then(left.0.cmp(&right.0)));
    ranked
        .into_iter()
        .take(TARGET_BUCKETS_PER_STAGE)
        .collect()
}

fn seed_priority(entry: &crate::book::BookEntry, ply: usize, stage: usize, bucket_count: usize) -> i64 {
    let center_band_bonus = (TARGET_SEED_PLY_MAX as i64 - (TARGET_SEED_PLY_MAX as i64 - ply as i64).abs()) * 20;
    let source_bonus = match entry.source {
        EntrySource::Book => 300,
        EntrySource::Verifier => 120,
        EntrySource::RuleProof => 80,
    };
    let verdict_bonus = match entry.verdict {
        Verdict::SolvedDraw => 220,
        Verdict::SolvedWin => 120,
        Verdict::Unresolved => 0,
    };
    let move_bonus = entry.best_moves.len() as i64 * 15;
    let stage_spread = (ply as i64 * (stage as i64 + 1)) % 97;
    let scarcity_bonus = (20_000usize.saturating_sub(bucket_count.min(20_000))) as i64 / 4;

    center_band_bonus + source_bonus + verdict_bonus + move_bonus + stage_spread + scarcity_bonus
}

fn principal_seed_paths(position: &Position, stage: usize) -> Vec<Vec<Column>> {
    (0..TARGET_VARIANTS_PER_SEED)
        .map(|variant| principal_seed_path_variant(position, stage, variant))
        .filter(|path| !path.is_empty())
        .collect()
}

fn principal_seed_path_variant(position: &Position, stage: usize, variant: usize) -> Vec<Column> {
    let mut path = Vec::new();
    let mut current = position.clone();
    let mut steps = 0usize;

    while steps < TARGET_DESCENT_STEPS {
        let moves = diversified_principal_moves(&current, stage, variant, steps);
        let Some(&next_move) = moves.first() else {
            break;
        };
        if current.apply_move(next_move).is_err() {
            break;
        }
        path.push(next_move);
        steps += 1;

        if 42usize.saturating_sub(current.move_count) <= VERIFIER_MAX_EMPTIES {
            break;
        }
    }

    path
}

fn diversified_principal_moves(position: &Position, stage: usize, variant: usize, step: usize) -> Vec<Column> {
    let mut moves = principal_line_moves(position);
    if moves.is_empty() {
        return moves;
    }

    let ordered = ordered_moves(position.legal_moves());
    let rotate_by = (stage + variant + step) % ordered.len().max(1);
    for offset in 0..ordered.len().min(3) {
        let alternate = ordered[(rotate_by + offset) % ordered.len()];
        if !moves.contains(&alternate) {
            moves.push(alternate);
        }
    }

    let selected_idx = if step == 0 {
        variant.min(moves.len().saturating_sub(1))
    } else if step == 1 && variant > 0 {
        (variant - 1).min(moves.len().saturating_sub(1))
    } else {
        0
    };
    let selected = moves.remove(selected_idx);
    moves.insert(0, selected);
    moves
}

fn descend_from_seed(
    state: &mut FrontierState,
    entries: &[BookEntry],
    seed_position: &Position,
    seed_path: &[Column],
    max_steps: usize,
) -> usize {
    let mut position = seed_position.clone();
    let mut path = seed_path.to_vec();
    let mut inserted = 0usize;

    maybe_enqueue(state, entries, path.clone(), &position, &mut inserted);
    if inserted >= RESEED_TARGET_NEW_ENTRIES {
        return inserted;
    }

    let mut steps = 0usize;
    while steps < max_steps && 42usize.saturating_sub(position.move_count) > VERIFIER_MAX_EMPTIES {
        let moves = principal_line_moves(&position);
        let Some(&next_move) = moves.first() else {
            break;
        };
        if position.apply_move(next_move).is_err() {
            break;
        }
        path.push(next_move);
        let enqueued = maybe_enqueue(state, entries, path.clone(), &position, &mut inserted);
        if inserted >= RESEED_TARGET_NEW_ENTRIES {
            break;
        }
        if !enqueued
            && 42usize.saturating_sub(position.move_count) <= VERIFIER_MAX_EMPTIES
            && entries
                .iter()
                .any(|entry| entry.canonical_key == position.board.canonical_key().to_vec() && entry.side_to_move == position.side_to_move)
        {
            break;
        }
        steps += 1;
    }

    inserted
}

fn maybe_enqueue(
    state: &mut FrontierState,
    entries: &[BookEntry],
    path: Vec<Column>,
    position: &Position,
    inserted: &mut usize,
) -> bool {
    if 42usize.saturating_sub(position.move_count) <= VERIFIER_MAX_EMPTIES
        && entries
            .iter()
            .any(|entry| entry.canonical_key == position.board.canonical_key().to_vec() && entry.side_to_move == position.side_to_move)
    {
        return false;
    }

    let key = position_key(position);
    if state.seen.insert(key) {
        state.pending.push(frontier_entry(path, position));
        *inserted += 1;
        true
    } else {
        false
    }
}

fn position_from_canonical_entry(entry: &crate::book::BookEntry) -> Option<Position> {
    position_from_canonical_state(&entry.canonical_key, entry.side_to_move)
}

fn position_from_canonical_state(canonical_key: &[u8], side_to_move: Color) -> Option<Position> {
    let mut white_cells = Vec::new();
    let mut black_cells = Vec::new();

    if canonical_key.len() != 42 {
        return None;
    }

    for (idx, &cell) in canonical_key.iter().enumerate() {
        let col = idx / 6;
        let row = idx % 6;
        match cell {
            1 => white_cells.push((row, col)),
            2 => black_cells.push((row, col)),
            _ => {}
        }
    }

    for col in 0..7 {
        let filled = canonical_key[(col * 6)..((col + 1) * 6)]
            .iter()
            .take_while(|&&cell| cell != 0)
            .count();
        if canonical_key[(col * 6)..((col * 6) + filled)].contains(&0) {
            return None;
        }
    }

    white_cells.sort_unstable();
    black_cells.sort_unstable();

    let mut position = Position::new();
    let mut next_white = 0usize;
    let mut next_black = 0usize;
    let total_moves = white_cells.len() + black_cells.len();

    while position.move_count < total_moves {
        let side = position.side_to_move;
        let target = match side {
            Color::White => *white_cells.get(next_white)?,
            Color::Black => *black_cells.get(next_black)?,
        };

        let playable = position.board.playable_cell(Column(target.1)).ok()?;
        if playable.row != target.0 + 1 {
            return None;
        }

        position.apply_move(Column(target.1)).ok()?;
        match side {
            Color::White => next_white += 1,
            Color::Black => next_black += 1,
        }
    }

    if position.board.canonical_key().as_slice() == canonical_key
        && position.side_to_move == side_to_move
    {
        Some(position)
    } else {
        None
    }
}

fn reseed_moves(position: &Position, entries: &[BookEntry], stage: usize) -> Vec<Column> {
    let mut ordered = ordered_moves(position.legal_moves());
    if ordered.is_empty() {
        return ordered;
    }

    if let Some(entry) = entries
        .iter()
        .find(|entry| entry.canonical_key == position.board.canonical_key().to_vec() && entry.side_to_move == position.side_to_move)
    {
        for &column in entry.best_moves.iter().rev() {
            if let Some(idx) = ordered.iter().position(|&candidate| candidate == column) {
                let best = ordered.remove(idx);
                ordered.insert(0, best);
            }
        }
    } else if let Some(search) = search_best_move(position, position.side_to_move, 6, 120_000)
        && let Some(best_idx) = ordered.iter().position(|&column| column == search.best_move)
    {
        let best = ordered.remove(best_idx);
        ordered.insert(0, best);
    }

    let cap = reseed_width_for_depth(position.move_count, stage);
    ordered.into_iter().take(cap).collect()
}

fn reseed_width_for_depth(depth: usize, stage: usize) -> usize {
    let stage_bonus = (stage / 2).min(2);
    match depth {
        0 => 7,
        1 => 4 + stage_bonus,
        2 | 3 => 3 + stage_bonus,
        4 | 5 => 2 + stage_bonus.min(1),
        _ => 1 + usize::from(stage >= 3 && depth <= 7),
    }
}

fn ply_bucket_start(ply: usize) -> usize {
    (ply / TARGET_BUCKET_WIDTH) * TARGET_BUCKET_WIDTH
}

fn checkpoint_due(processed: usize, checkpoint_every: usize, batch_limit: usize) -> bool {
    checkpoint_every > 0 && processed > 0 && processed < batch_limit && processed.is_multiple_of(checkpoint_every)
}

fn flush_book_writes(store: &RedbBookStore, pending: &mut Vec<BookEntry>) -> Result<(), String> {
    store.insert_batch(pending).map_err(|err| err.to_string())?;
    pending.clear();
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn certify_exact_value(
    store: &RedbBookStore,
    root: &Position,
    position: &Position,
    target: Color,
    root_child: Option<Column>,
    root_grandchild: Option<Column>,
    ctx: &mut CertifyCtx,
    witness: &mut Vec<Column>,
    pending_book_writes: &mut Vec<BookEntry>,
) -> Result<Option<ExactValue>, String> {
    if ctx.nodes >= ctx.max_nodes {
        return Ok(None);
    }
    ctx.nodes += 1;
    maybe_report_certify_progress(ctx);

    if let Some(winner) = position.winner {
        return Ok(Some(if winner == target {
            ExactValue::Win
        } else {
            ExactValue::Loss
        }));
    }
    if position.is_draw() {
        return Ok(Some(ExactValue::Draw));
    }

    let cache_key = format!(
        "{}:{}",
        position_key(position),
        if target == Color::White { 'w' } else { 'b' }
    );
    if let Some(cached) = ctx.cache.get(&cache_key) {
        return Ok(*cached);
    }

    if let Some((exact_value, best_move)) = store
        .get_certify_cache(position, target)
        .map_err(|err| err.to_string())?
    {
        ctx.certify_cache_hits += 1;
        if let Some(best_move) = best_move {
            witness.clear();
            witness.push(best_move);
        }
        ctx.cache.insert(cache_key, Some(exact_value));
        return Ok(Some(exact_value));
    }

    let tt_move = store
        .get_certify_tt_move(position, target)
        .map_err(|err| err.to_string())?;
    if tt_move.is_some() {
        ctx.certify_tt_hits += 1;
    }
    let mut node_state = store
        .get_certify_node_state(position, target)
        .map_err(|err| err.to_string())?
        .unwrap_or_else(|| empty_certify_node_state(position));

    if let Some(entry) = store.get(position).map_err(|err| err.to_string())?
        && let Some(exact_value) = entry.exact_value
    {
        ctx.book_hits += 1;
        let value = target_value_to_side_value(position, target, exact_value);
        ctx.cache.insert(cache_key, Some(value));
        return Ok(Some(value));
    }

    let empties = 42usize.saturating_sub(position.move_count);
    if empties <= VERIFIER_MAX_EMPTIES {
        let remaining_budget = ctx.max_nodes.saturating_sub(ctx.nodes);
        if remaining_budget == 0 {
            ctx.cache.insert(cache_key, None);
            return Ok(None);
        }
        let outcome = verify_with_outcome(
            position,
            target,
            SearchConfig {
                max_nodes: remaining_budget,
                max_empties: VERIFIER_MAX_EMPTIES,
                iterative_deepening: true,
            },
        );
        ctx.nodes = ctx.nodes.saturating_add(outcome.nodes_visited);
        maybe_report_certify_progress(ctx);
        if let Some(result) = outcome.result {
            ctx.verifier_hits += 1;
            let best_moves = result
                .witness
                .as_ref()
                .and_then(|line| line.first().copied())
                .map(|column| vec![column])
                .unwrap_or_default();
            let exact_for_side = target_value_to_side_value(position, target, result.verdict);
            pending_book_writes.push(verifier_entry_for_position(position, exact_for_side, best_moves));
            *witness = result.witness.unwrap_or_default();
            let _ = store.insert_certify_cache(position, target, result.verdict, witness.first().copied());
            if let Some(best_move) = witness.first().copied() {
                let _ = store.insert_certify_tt_move(position, target, best_move);
            }
            let _ = store.remove_certify_frontier(root, target, position);
            ctx.cache.insert(cache_key, Some(result.verdict));
            return Ok(Some(result.verdict));
        }
        if let Some(best_move) = tt_move {
            let _ = store.insert_certify_tt_move(position, target, best_move);
        }
        if should_persist_certify_frontier(root_child, root_grandchild) {
            let _ = store.insert_certify_frontier(root, target, position, root_child, root_grandchild);
        }
        ctx.cache.insert(cache_key, None);
        return Ok(None);
    }

    let maximizing = position.side_to_move == target;
    let mut best_value = if maximizing {
        ExactValue::Loss
    } else {
        ExactValue::Win
    };
    let mut best_line = Vec::new();
    let mut saw_unresolved = false;

    if let Some(derived) = derive_exact_from_node_state(position, target, &node_state) {
        if let Some(best_move) = node_state.best_move {
            witness.clear();
            witness.push(best_move);
        }
        ctx.cache.insert(cache_key, Some(derived));
        return Ok(Some(derived));
    }

    for column in certification_moves(store, position, target, tt_move, &node_state) {
        let mut child = position.clone();
        child.apply_move(column)
            .map_err(|err| format!("cannot certify child move: {err}"))?;
        let mut child_line = Vec::new();
        let Some(child_value) = certify_exact_value(
            store,
            root,
            &child,
            target,
            root_child.or(Some(column)),
            root_grandchild.or_else(|| root_child.map(|_| column)),
            ctx,
            &mut child_line,
            pending_book_writes,
        )?
        else {
            saw_unresolved = true;
            update_child_status(&mut node_state, column, CertifyChildStatus::AttemptedUnresolved);
            continue;
        };

        update_child_status(&mut node_state, column, child_status_from_exact(child_value));

        if maximizing {
            if exact_rank(child_value) > exact_rank(best_value) {
                best_value = child_value;
                best_line = child_line;
                best_line.insert(0, column);
            }
            if best_value == ExactValue::Win {
                break;
            }
        } else {
            if exact_rank(child_value) < exact_rank(best_value) {
                best_value = child_value;
                best_line = child_line;
                best_line.insert(0, column);
            }
            if best_value == ExactValue::Loss {
                break;
            }
        }
    }

    if saw_unresolved
        && ((maximizing && best_value != ExactValue::Win) || (!maximizing && best_value != ExactValue::Loss))
    {
        if let Some(best_move) = best_line.first().copied().or(tt_move) {
            node_state.best_move = Some(best_move);
            let _ = store.insert_certify_tt_move(position, target, best_move);
        }
        let _ = store.insert_certify_node_state(position, target, &node_state);
        if should_persist_certify_frontier(root_child, root_grandchild) {
            let _ = store.insert_certify_frontier(root, target, position, root_child, root_grandchild);
        }
        ctx.cache.insert(cache_key, None);
        return Ok(None);
    }

    *witness = best_line;
    node_state.best_move = witness.first().copied();
    let _ = store.insert_certify_cache(position, target, best_value, witness.first().copied());
    if let Some(best_move) = witness.first().copied() {
        let _ = store.insert_certify_tt_move(position, target, best_move);
    }
    let _ = store.insert_certify_node_state(position, target, &node_state);
    let _ = store.remove_certify_frontier(root, target, position);
    ctx.cache.insert(cache_key, Some(best_value));
    Ok(Some(best_value))
}

fn should_persist_certify_frontier(
    root_child: Option<Column>,
    root_grandchild: Option<Column>,
) -> bool {
    root_child.is_none() || root_grandchild.is_some()
}

fn process_persistent_certify_frontier(
    store: &RedbBookStore,
    root: &Position,
    target: Color,
    ctx: &mut CertifyCtx,
    pending_book_writes: &mut Vec<BookEntry>,
    frontier_budget: usize,
) -> Result<(), String> {
    let frontier_budget_end = ctx.nodes.saturating_add(frontier_budget).min(ctx.max_nodes);
    while ctx.nodes < frontier_budget_end {
        let blockers = unresolved_root_children(store, root, target)?;
        let blocker_order = blockers.iter().map(|report| report.column).collect::<Vec<_>>();
        let mut entries = store
            .list_certify_frontier(root, target, usize::MAX)
            .map_err(|err| err.to_string())?;
        if entries.is_empty() {
            break;
        }
        sort_certify_frontier_entries(&mut entries, &blocker_order);
        entries.truncate(128);
        update_frontier_empties_stats(ctx, &entries);

        let mut made_progress = false;
        for entry in entries {
            if ctx.nodes >= frontier_budget_end {
                break;
            }
            let Some(position) = position_from_canonical_state(&entry.canonical_key, entry.side_to_move) else {
                continue;
            };
            ctx.certify_frontier_resumed += 1;

            let before_nodes = ctx.nodes;
            let mut witness = Vec::new();
            let result = certify_exact_value(
                store,
                root,
                &position,
                target,
                entry.root_child,
                entry.root_grandchild,
                ctx,
                &mut witness,
                pending_book_writes,
            )?;
            if result.is_some() {
                let _ = store.remove_certify_frontier(root, target, &position);
                ctx.certify_frontier_solved += 1;
            } else {
                ctx.certify_frontier_requeued += 1;
            }
            if ctx.nodes > before_nodes || result.is_some() {
                made_progress = true;
            }
        }

        if !made_progress {
            break;
        }
    }
    Ok(())
}

fn process_root_child_blockers(
    store: &RedbBookStore,
    root: &Position,
    target: Color,
    ctx: &mut CertifyCtx,
    pending_book_writes: &mut Vec<BookEntry>,
    blocker_budget: usize,
) -> Result<(), String> {
    let budget_end = ctx.nodes.saturating_add(blocker_budget).min(ctx.max_nodes);
    while ctx.nodes < budget_end {
        let blockers = active_root_grandchild_targets(store, root, target)?;
        if blockers.is_empty() {
            break;
        }

        let per_child_budget = blocker_slice_budget(ctx.nodes, budget_end, blockers.len());
        let mut made_progress = false;
        for blocker in blockers {
            if ctx.nodes >= budget_end {
                break;
            }
            let child_budget_end = ctx
                .nodes
                .saturating_add(per_child_budget)
                .min(budget_end)
                .max(ctx.nodes.saturating_add(1));
            let mut child = root.clone();
            child.apply_move(blocker.root_child)
                .map_err(|err| format!("cannot analyze root child blocker: {err}"))?;
            if let Some(grandchild) = blocker.grandchild {
                child.apply_move(grandchild)
                    .map_err(|err| format!("cannot analyze root grandchild blocker: {err}"))?;
            }
            while ctx.nodes < child_budget_end {
                let before_nodes = ctx.nodes;
                let mut witness = Vec::new();
                let _ = certify_exact_value(
                    store,
                    root,
                    &child,
                    target,
                    Some(blocker.root_child),
                    blocker.grandchild,
                    ctx,
                    &mut witness,
                    pending_book_writes,
                )?;
                if ctx.nodes > before_nodes {
                    made_progress = true;
                } else {
                    break;
                }
            }
        }

        if !made_progress {
            break;
        }
    }
    Ok(())
}

fn blocker_slice_budget(current_nodes: usize, budget_end: usize, blocker_count: usize) -> usize {
    if blocker_count == 0 {
        return 0;
    }
    let remaining = budget_end.saturating_sub(current_nodes);
    let even_share = remaining / blocker_count.max(1);
    even_share.clamp(200_000, 2_000_000).min(remaining.max(1))
}

fn active_root_grandchild_targets(
    store: &RedbBookStore,
    root: &Position,
    target: Color,
) -> Result<Vec<RootGrandchildTarget>, String> {
    let reports = inspect_certify_root_state_from_store(store, root, target)?;
    let mut targets = Vec::new();
    for child in reports {
        if child.exact_value.is_some() {
            continue;
        }
        if child.child_statuses.is_empty() {
            targets.push(RootGrandchildTarget {
                root_child: child.column,
                grandchild: None,
                attempted_unresolved: child.attempted_unresolved_children > 0,
                frontier_count: child.frontier_count,
                frontier_min_empties: child.frontier_min_empties,
                known_children: child.known_children,
            });
            continue;
        }
        for grandchild in child.child_statuses {
            targets.push(RootGrandchildTarget {
                root_child: child.column,
                grandchild: Some(grandchild.column),
                attempted_unresolved: grandchild.status == CertifyChildStatus::AttemptedUnresolved,
                frontier_count: grandchild.frontier_count,
                frontier_min_empties: grandchild.frontier_min_empties,
                known_children: child.known_children,
            });
        }
    }
    targets.sort_by(|left, right| {
        left
            .attempted_unresolved
            .cmp(&right.attempted_unresolved)
            .then(left.frontier_count.cmp(&right.frontier_count))
            .then(left.frontier_min_empties.cmp(&right.frontier_min_empties))
            .then(right.known_children.cmp(&left.known_children))
            .then(left.root_child.0.cmp(&right.root_child.0))
            .then(left.grandchild.map(|c| c.0).cmp(&right.grandchild.map(|c| c.0)))
    });
    targets.truncate(4);
    Ok(targets)
}

fn unresolved_root_children(
    store: &RedbBookStore,
    root: &Position,
    target: Color,
) -> Result<Vec<RootChildReport>, String> {
    let mut reports = analyze_root_children(store, root, target)?
        .into_iter()
        .filter(|report| report.exact_value.is_none())
        .collect::<Vec<_>>();
    reports.sort_by(|left, right| {
        right
            .known_children
            .cmp(&left.known_children)
            .then(
                right
                    .attempted_unresolved_children
                    .cmp(&left.attempted_unresolved_children),
            )
            .then(left.frontier_count.cmp(&right.frontier_count))
            .then(left.frontier_min_empties.cmp(&right.frontier_min_empties))
            .then(left.total_children.cmp(&right.total_children))
            .then(left.column.0.cmp(&right.column.0))
    });
    Ok(reports)
}

fn analyze_root_children(
    store: &RedbBookStore,
    root: &Position,
    target: Color,
) -> Result<Vec<RootChildReport>, String> {
    let frontier_entries = store
        .list_certify_frontier(root, target, usize::MAX)
        .map_err(|err| err.to_string())?;
    let mut frontier_by_root_child = BTreeMap::<usize, (usize, Option<usize>)>::new();
    for entry in frontier_entries {
        if let Some(root_child) = entry.root_child {
            let stats = frontier_by_root_child.entry(root_child.0).or_insert((0, None));
            stats.0 += 1;
            stats.1 = Some(
                stats
                    .1
                    .map_or(entry.empties, |current: usize| current.min(entry.empties)),
            );
        }
    }

    let mut reports = Vec::new();
    for column in root.legal_moves() {
        let mut child = root.clone();
        child.apply_move(column)
            .map_err(|err| format!("cannot inspect root child: {err}"))?;
        let exact_value = persisted_exact_value(store, &child, target)?;
        let node_state = store
            .get_certify_node_state(&child, target)
            .map_err(|err| err.to_string())?;
        let (known_children, attempted_unresolved_children, total_children) = node_state
            .as_ref()
            .map(|state| {
                (
                    state
                        .children
                        .iter()
                        .filter(|child| {
                            matches!(
                                child.status,
                                CertifyChildStatus::ExactWin
                                    | CertifyChildStatus::ExactDraw
                                    | CertifyChildStatus::ExactLoss
                            )
                        })
                        .count(),
                    state
                        .children
                        .iter()
                        .filter(|child| child.status == CertifyChildStatus::AttemptedUnresolved)
                        .count(),
                    state.children.len(),
                )
            })
            .unwrap_or_else(|| (0, 0, child.legal_moves().len()));
        let (frontier_count, frontier_min_empties) = frontier_by_root_child
            .get(&column.0)
            .copied()
            .unwrap_or((0, None));

        reports.push(RootChildReport {
            column,
            exact_value,
            known_children,
            attempted_unresolved_children,
            total_children,
            frontier_count,
            frontier_min_empties,
        });
    }
    Ok(reports)
}

fn persisted_exact_value(
    store: &RedbBookStore,
    position: &Position,
    target: Color,
) -> Result<Option<ExactValue>, String> {
    if let Some((exact_value, _)) = store
        .get_certify_cache(position, target)
        .map_err(|err| err.to_string())?
    {
        return Ok(Some(exact_value));
    }
    if let Some(entry) = store.get(position).map_err(|err| err.to_string())?
        && let Some(exact_value) = entry.exact_value
    {
        return Ok(Some(target_value_to_side_value(position, target, exact_value)));
    }
    if let Some(state) = store
        .get_certify_node_state(position, target)
        .map_err(|err| err.to_string())?
    {
        return Ok(derive_exact_from_node_state(position, target, &state));
    }
    Ok(None)
}

fn sort_certify_frontier_entries(
    entries: &mut [crate::bookdb::CertifyFrontierEntry],
    blocker_order: &[Column],
) {
    entries.sort_by(|left, right| {
        root_child_priority(left.root_child, blocker_order)
            .cmp(&root_child_priority(right.root_child, blocker_order))
            .then(left.empties.cmp(&right.empties))
            .then_with(|| {
                let left_pos = position_from_canonical_state(&left.canonical_key, left.side_to_move);
                let right_pos = position_from_canonical_state(&right.canonical_key, right.side_to_move);
                let left_key = left_pos
                    .as_ref()
                    .map(|position| {
                        let threat = immediate_wins(position, position.side_to_move)
                            + immediate_wins(position, position.side_to_move.opponent());
                        let branching = position.legal_moves().len();
                        (std::cmp::Reverse(threat), branching)
                    })
                    .unwrap_or((std::cmp::Reverse(0usize), usize::MAX));
                let right_key = right_pos
                    .as_ref()
                    .map(|position| {
                        let threat = immediate_wins(position, position.side_to_move)
                            + immediate_wins(position, position.side_to_move.opponent());
                        let branching = position.legal_moves().len();
                        (std::cmp::Reverse(threat), branching)
                    })
                    .unwrap_or((std::cmp::Reverse(0usize), usize::MAX));
                left_key.cmp(&right_key)
            })
    });
}

fn root_child_priority(root_child: Option<Column>, blocker_order: &[Column]) -> usize {
    match root_child {
        Some(column) => blocker_order
            .iter()
            .position(|&candidate| candidate == column)
            .unwrap_or(blocker_order.len()),
        None => blocker_order.len() + 1,
    }
}

fn update_frontier_empties_stats(
    ctx: &mut CertifyCtx,
    entries: &[crate::bookdb::CertifyFrontierEntry],
) {
    if let Some(min_entry) = entries.iter().min_by_key(|entry| entry.empties) {
        ctx.certify_frontier_min_empties = Some(
            ctx.certify_frontier_min_empties
                .map_or(min_entry.empties, |current| current.min(min_entry.empties)),
        );
    }
    if let Some(max_entry) = entries.iter().max_by_key(|entry| entry.empties) {
        ctx.certify_frontier_max_empties = Some(
            ctx.certify_frontier_max_empties
                .map_or(max_entry.empties, |current| current.max(max_entry.empties)),
        );
    }
}

fn maybe_report_certify_progress(ctx: &mut CertifyCtx) {
    if !ctx.progress_enabled || ctx.max_nodes == 0 {
        return;
    }

    let percent = (ctx.nodes.saturating_mul(100) / ctx.max_nodes).min(100);
    if percent < ctx.next_progress_percent && ctx.nodes < ctx.max_nodes {
        return;
    }

    render_certify_progress(ctx.nodes, ctx.max_nodes, ctx.started_at);

    while ctx.next_progress_percent <= percent {
        ctx.next_progress_percent += 5;
    }
}

fn finish_certify_progress(ctx: &CertifyCtx) {
    if ctx.progress_enabled {
        eprintln!();
    }
}

fn start_certify_progress(ctx: &CertifyCtx) {
    if ctx.progress_enabled {
        render_certify_progress(0, ctx.max_nodes, ctx.started_at);
    }
}

fn render_certify_progress(nodes: usize, max_nodes: usize, started_at: Option<Instant>) {
    let percent = if max_nodes == 0 {
        100
    } else {
        (nodes.saturating_mul(100) / max_nodes).min(100)
    };
    let filled = (percent / 5).min(20);
    let bar = format!(
        "[{}{}]",
        "#".repeat(filled),
        ".".repeat(20usize.saturating_sub(filled))
    );
    let elapsed = started_at.map(|started| started.elapsed()).unwrap_or_default();
    let elapsed_secs = elapsed.as_secs_f64();
    let nodes_per_sec = if nodes == 0 || elapsed_secs < 0.25 {
        0.0
    } else {
        nodes as f64 / elapsed_secs
    };
    let remaining_nodes = max_nodes.saturating_sub(nodes);
    let eta_secs = if nodes_per_sec > 0.0 {
        (remaining_nodes as f64 / nodes_per_sec).round() as u64
    } else {
        0
    };
    eprint!(
        "\rcertify progress {bar} {percent:>3}% ({}/{})  {} n/s  elapsed {}  eta {}",
        nodes,
        max_nodes,
        format_rate(nodes_per_sec),
        format_duration(elapsed.as_secs()),
        format_duration(eta_secs),
    );
    let _ = io::stderr().flush();
}

fn format_rate(nodes_per_sec: f64) -> String {
    if nodes_per_sec >= 1_000_000.0 {
        format!("{:.1}M", nodes_per_sec / 1_000_000.0)
    } else if nodes_per_sec >= 1_000.0 {
        format!("{:.1}k", nodes_per_sec / 1_000.0)
    } else {
        format!("{:.0}", nodes_per_sec)
    }
}

fn format_duration(total_secs: u64) -> String {
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

fn certification_moves(
    store: &RedbBookStore,
    position: &Position,
    target: Color,
    tt_move: Option<Column>,
    node_state: &CertifyNodeState,
) -> Vec<Column> {
    let mut ordered = ordered_moves(position.legal_moves());
    if ordered.is_empty() {
        return ordered;
    }

    if let Some(tt_move) = tt_move
        && let Some(idx) = ordered.iter().position(|&candidate| candidate == tt_move)
    {
        let best = ordered.remove(idx);
        ordered.insert(0, best);
        return ordered;
    }

    ordered.sort_by_key(|&column| certification_child_priority(position, target, node_state, column));

    if let Ok(Some(entry)) = store.get(position) {
        for &column in entry.best_moves.iter().rev() {
            if let Some(idx) = ordered.iter().position(|&candidate| candidate == column) {
                let best = ordered.remove(idx);
                ordered.insert(0, best);
            }
        }
        return ordered;
    }

    if let Some(search) = search_best_move(position, position.side_to_move, 6, 120_000)
        && let Some(best_idx) = ordered.iter().position(|&column| column == search.best_move)
    {
        let best = ordered.remove(best_idx);
        ordered.insert(0, best);
    }

    ordered
}

fn empty_certify_node_state(position: &Position) -> CertifyNodeState {
    CertifyNodeState {
        best_move: None,
        children: position
            .legal_moves()
            .into_iter()
            .map(|column| CertifyChildRecord {
                column,
                status: CertifyChildStatus::Unknown,
            })
            .collect(),
    }
}

fn update_child_status(state: &mut CertifyNodeState, column: Column, status: CertifyChildStatus) {
    if let Some(child) = state.children.iter_mut().find(|child| child.column == column) {
        child.status = status;
    } else {
        state.children.push(CertifyChildRecord { column, status });
    }
}

fn child_status_from_exact(value: ExactValue) -> CertifyChildStatus {
    match value {
        ExactValue::Win => CertifyChildStatus::ExactWin,
        ExactValue::Draw => CertifyChildStatus::ExactDraw,
        ExactValue::Loss => CertifyChildStatus::ExactLoss,
    }
}

fn certification_child_priority(
    position: &Position,
    target: Color,
    state: &CertifyNodeState,
    column: Column,
) -> (usize, i32) {
    let maximizing = position.side_to_move == target;
    let status_priority = state
        .children
        .iter()
        .find(|child| child.column == column)
        .map(|child| match (maximizing, child.status) {
            (true, CertifyChildStatus::ExactWin) => 0,
            (true, CertifyChildStatus::ExactDraw) => 1,
            (true, CertifyChildStatus::Unknown) => 2,
            (true, CertifyChildStatus::AttemptedUnresolved) => 3,
            (true, CertifyChildStatus::ExactLoss) => 4,
            (false, CertifyChildStatus::ExactLoss) => 0,
            (false, CertifyChildStatus::ExactDraw) => 1,
            (false, CertifyChildStatus::Unknown) => 2,
            (false, CertifyChildStatus::AttemptedUnresolved) => 3,
            (false, CertifyChildStatus::ExactWin) => 4,
        })
        .unwrap_or(2);
    (status_priority, (3_i32 - column.0 as i32).abs())
}

fn derive_exact_from_node_state(
    position: &Position,
    target: Color,
    state: &CertifyNodeState,
) -> Option<ExactValue> {
    if state.children.is_empty() || state.children.iter().any(|child| {
        matches!(
            child.status,
            CertifyChildStatus::Unknown | CertifyChildStatus::AttemptedUnresolved
        )
    }) {
        return None;
    }

    let values = state.children.iter().map(|child| match child.status {
        CertifyChildStatus::ExactWin => ExactValue::Win,
        CertifyChildStatus::ExactDraw => ExactValue::Draw,
        CertifyChildStatus::ExactLoss => ExactValue::Loss,
        CertifyChildStatus::Unknown | CertifyChildStatus::AttemptedUnresolved => unreachable!(),
    });

    if position.side_to_move == target {
        values.max_by_key(|value| exact_rank(*value))
    } else {
        values.min_by_key(|value| exact_rank(*value))
    }
}

fn target_value_to_side_value(position: &Position, target: Color, value: ExactValue) -> ExactValue {
    if position.side_to_move == target {
        value
    } else {
        invert_exact_value(value)
    }
}

fn propagate_branch_root_entries(
    store: &RedbBookStore,
    seed_moves: &[Column],
) -> Result<Option<(usize, usize)>, String> {
    let mut inserted = 0usize;
    let mut updated = 0usize;

    for prefix_len in (1..=seed_moves.len()).rev() {
        let position = position_from_moves(&seed_moves[..prefix_len])
            .map_err(|err| format!("invalid branch prefix: {err}"))?;
        let already_exact = store
            .get(&position)
            .map_err(|err| err.to_string())?
            .and_then(|entry| entry.exact_value)
            .is_some();
        if already_exact {
            continue;
        }

        if let Some(result) = solve_from_book_cache(
            store,
            &position,
            position.side_to_move,
            &mut BranchSolveCtx::default(),
        )? {
            let best_moves = result
                .witness
                .as_ref()
                .and_then(|line| line.first().copied())
                .map(|column| vec![column])
                .unwrap_or_default();
            let existed = store.get(&position).map_err(|err| err.to_string())?.is_some();
            store
                .insert(&verifier_entry_for_position(&position, result.verdict, best_moves))
                .map_err(|err| err.to_string())?;
            if existed {
                updated += 1;
            } else {
                inserted += 1;
            }
        }
    }

    if inserted == 0 && updated == 0 {
        Ok(None)
    } else {
        Ok(Some((inserted, updated)))
    }
}

#[derive(Default)]
struct BranchSolveCtx {
    nodes: usize,
    cache: std::collections::HashMap<String, Option<ExactValue>>,
}

fn solve_from_book_cache(
    store: &RedbBookStore,
    position: &Position,
    target: Color,
    ctx: &mut BranchSolveCtx,
) -> Result<Option<crate::verifier::VerifierResult>, String> {
    const MAX_BRANCH_PROPAGATION_NODES: usize = 2_000_000;
    let mut witness = Vec::new();
    let value = solve_from_book_cache_inner(store, position, target, ctx, &mut witness, MAX_BRANCH_PROPAGATION_NODES)?;
    Ok(value.map(|verdict| crate::verifier::VerifierResult {
        verdict,
        nodes_visited: ctx.nodes,
        witness: Some(witness),
        reason: "branch back-propagation from exact cached descendants",
        cache_hits: 0,
        cutoffs: 0,
    }))
}

fn solve_from_book_cache_inner(
    store: &RedbBookStore,
    position: &Position,
    target: Color,
    ctx: &mut BranchSolveCtx,
    witness: &mut Vec<Column>,
    max_nodes: usize,
) -> Result<Option<ExactValue>, String> {
    if ctx.nodes >= max_nodes {
        return Ok(None);
    }
    ctx.nodes += 1;

    if let Some(winner) = position.winner {
        return Ok(Some(if winner == target {
            ExactValue::Win
        } else {
            ExactValue::Loss
        }));
    }
    if position.is_draw() {
        return Ok(Some(ExactValue::Draw));
    }

    let key = position_key(position);
    if let Some(cached) = ctx.cache.get(&key) {
        return Ok(*cached);
    }

    if let Some(entry) = store.get(position).map_err(|err| err.to_string())?
        && let Some(exact) = entry.exact_value
    {
        let value = if position.side_to_move == target {
            exact
        } else {
            invert_exact_value(exact)
        };
        ctx.cache.insert(key, Some(value));
        return Ok(Some(value));
    }

    let maximizing = position.side_to_move == target;
    let mut best = if maximizing { ExactValue::Loss } else { ExactValue::Win };
    let mut best_line = Vec::new();

    for column in ordered_moves(position.legal_moves()) {
        let mut child = position.clone();
        child.apply_move(column)
            .map_err(|err| format!("cannot evaluate propagated child: {err}"))?;
        let mut child_line = Vec::new();
        let Some(child_value) = solve_from_book_cache_inner(store, &child, target, ctx, &mut child_line, max_nodes)? else {
            ctx.cache.insert(position_key(position), None);
            return Ok(None);
        };

        if maximizing {
            if exact_rank(child_value) > exact_rank(best) {
                best = child_value;
                best_line = child_line;
                best_line.insert(0, column);
            }
            if best == ExactValue::Win {
                break;
            }
        } else {
            if exact_rank(child_value) < exact_rank(best) {
                best = child_value;
                best_line = child_line;
                best_line.insert(0, column);
            }
            if best == ExactValue::Loss {
                break;
            }
        }
    }

    *witness = best_line;
    ctx.cache.insert(position_key(position), Some(best));
    Ok(Some(best))
}

fn invert_exact_value(value: ExactValue) -> ExactValue {
    match value {
        ExactValue::Win => ExactValue::Loss,
        ExactValue::Draw => ExactValue::Draw,
        ExactValue::Loss => ExactValue::Win,
    }
}

fn exact_rank(value: ExactValue) -> i8 {
    match value {
        ExactValue::Loss => -1,
        ExactValue::Draw => 0,
        ExactValue::Win => 1,
    }
}
