use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::board::{COLUMNS, Color, Column, Position, ROWS};
use crate::solver::{Proof, Verdict};

#[derive(Debug, Clone)]
pub struct VerifierResult {
    pub verdict: ExactValue,
    pub nodes_visited: usize,
    pub witness: Option<Vec<Column>>,
    pub reason: &'static str,
    pub cache_hits: usize,
    pub cutoffs: usize,
}

#[derive(Debug, Clone)]
pub struct VerifyOutcome {
    pub result: Option<VerifierResult>,
    pub nodes_visited: usize,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub best_move: Column,
    pub score: i16,
    pub nodes_visited: usize,
    pub cache_hits: usize,
    pub cutoffs: usize,
    pub depth_reached: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExactValue {
    Win,
    Draw,
    Loss,
}

impl ExactValue {
    fn from_score(score: i8) -> Self {
        match score {
            1 => Self::Win,
            0 => Self::Draw,
            -1 => Self::Loss,
            _ => unreachable!("score must be -1, 0 or 1"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SearchConfig {
    pub max_nodes: usize,
    pub max_empties: usize,
    pub iterative_deepening: bool,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            max_nodes: 300_000,
            max_empties: 14,
            iterative_deepening: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PositionKey {
    board_key: [u8; COLUMNS * ROWS],
    side_to_move: Color,
}

#[derive(Debug, Clone, Copy)]
enum Bound {
    Exact,
    Lower,
    Upper,
}

#[derive(Debug, Clone, Copy)]
struct CacheEntry {
    score: i16,
    best_move: Option<Column>,
    depth: usize,
    bound: Bound,
}

#[derive(Debug, Default)]
struct SearchCtx {
    nodes: usize,
    cache_hits: usize,
    cutoffs: usize,
    cache: HashMap<PositionKey, CacheEntry>,
}

pub fn verify(position: &Position) -> Option<VerifierResult> {
    verify_with_config(position, position.side_to_move, SearchConfig::default())
}

pub fn search_best_move(
    position: &Position,
    target: Color,
    depth_limit: usize,
    max_nodes: usize,
) -> Option<SearchResult> {
    if position.legal_moves().is_empty() {
        return None;
    }

    let mut ctx = SearchCtx::default();
    let mut best_move = None;
    let mut best_score = -SCORE_WIN;
    let max_depth = depth_limit.max(1);

    for depth in 1..=max_depth {
        let score = solve(
            position,
            target,
            depth,
            -SCORE_WIN,
            SCORE_WIN,
            &SearchConfig {
                max_nodes,
                max_empties: COLUMNS * ROWS,
                iterative_deepening: false,
            },
            &mut ctx,
        )?;

        let key = PositionKey {
            board_key: position.board.canonical_key(),
            side_to_move: position.side_to_move,
        };
        let entry = ctx.cache.get(&key).copied()?;
        if let Some(column) = entry.best_move {
            best_move = Some(column);
            best_score = score;
        }
    }

    Some(SearchResult {
        best_move: best_move?,
        score: best_score,
        nodes_visited: ctx.nodes,
        cache_hits: ctx.cache_hits,
        cutoffs: ctx.cutoffs,
        depth_reached: max_depth,
    })
}

pub fn verify_for_player(position: &Position, target: Color, max_nodes: usize) -> Option<ExactValue> {
    verify_with_config(
        position,
        target,
        SearchConfig {
            max_nodes,
            ..SearchConfig::default()
        },
    )
    .map(|result| result.verdict)
}

pub fn verify_with_config(
    position: &Position,
    target: Color,
    config: SearchConfig,
) -> Option<VerifierResult> {
    verify_with_outcome(position, target, config).result
}

pub fn verify_with_outcome(
    position: &Position,
    target: Color,
    config: SearchConfig,
) -> VerifyOutcome {
    let empties = COLUMNS * ROWS - position.move_count;
    if empties > config.max_empties {
        return VerifyOutcome {
            result: None,
            nodes_visited: 0,
        };
    }

    let mut ctx = SearchCtx::default();
    let final_depth = empties;

    if config.iterative_deepening {
        for depth in 1..final_depth {
            if solve(position, target, depth, -SCORE_WIN, SCORE_WIN, &config, &mut ctx).is_none() {
                return VerifyOutcome {
                    result: None,
                    nodes_visited: ctx.nodes,
                };
            }
        }
    }

    let Some(score) = solve(
        position,
        target,
        final_depth,
        -SCORE_WIN,
        SCORE_WIN,
        &config,
        &mut ctx,
    ) else {
        return VerifyOutcome {
            result: None,
            nodes_visited: ctx.nodes,
        };
    };
    let witness = principal_variation(position, target, &ctx.cache);

    VerifyOutcome {
        result: Some(VerifierResult {
            verdict: ExactValue::from_score(score.signum() as i8),
            nodes_visited: ctx.nodes,
            witness,
            reason: "alpha-beta closure search",
            cache_hits: ctx.cache_hits,
            cutoffs: ctx.cutoffs,
        }),
        nodes_visited: ctx.nodes,
    }
}

pub fn proof_from_verifier(result: &VerifierResult) -> Proof {
    Proof {
        verdict: match result.verdict {
            ExactValue::Win => Verdict::SolvedWin,
            ExactValue::Draw | ExactValue::Loss => Verdict::SolvedDraw,
        },
        chosen_solution_ids: Vec::new(),
        unresolved_problem_ids: Vec::new(),
    }
}

fn solve(
    position: &Position,
    target: Color,
    depth: usize,
    mut alpha: i16,
    mut beta: i16,
    config: &SearchConfig,
    ctx: &mut SearchCtx,
) -> Option<i16> {
    if ctx.nodes >= config.max_nodes {
        return None;
    }
    ctx.nodes += 1;

    if let Some(winner) = position.winner {
        return Some(if winner == target { SCORE_WIN } else { -SCORE_WIN });
    }
    if position.is_draw() {
        return Some(0);
    }
    if depth == 0 {
        return Some(evaluate_non_terminal(position, target));
    }

    let key = PositionKey {
        board_key: position.board.canonical_key(),
        side_to_move: position.side_to_move,
    };
    if let Some(entry) = ctx.cache.get(&key).copied()
        && entry.depth >= depth
    {
        match entry.bound {
            Bound::Exact => {
                ctx.cache_hits += 1;
                return Some(entry.score);
            }
            Bound::Lower if entry.score >= beta => {
                ctx.cache_hits += 1;
                return Some(entry.score);
            }
            Bound::Upper if entry.score <= alpha => {
                ctx.cache_hits += 1;
                return Some(entry.score);
            }
            _ => {}
        }
    }

    let alpha_orig = alpha;
    let beta_orig = beta;
    let maximizing = position.side_to_move == target;
    let mut best_score = if maximizing { -SCORE_WIN } else { SCORE_WIN };
    let mut best_move = None;
    let tt_move = ctx.cache.get(&key).and_then(|entry| entry.best_move);
    let ordered = ordered_moves(position, target, tt_move);

    for column in ordered {
        let mut child = position.clone();
        child.apply_move(column).ok()?;

        let score = solve(&child, target, depth - 1, alpha, beta, config, ctx)?;

        if maximizing {
            if score > best_score {
                best_score = score;
                best_move = Some(column);
            }
            alpha = alpha.max(best_score);
        } else {
            if score < best_score {
                best_score = score;
                best_move = Some(column);
            }
            beta = beta.min(best_score);
        }

        if beta <= alpha {
            ctx.cutoffs += 1;
            break;
        }
    }

    let bound = if best_score <= alpha_orig {
        Bound::Upper
    } else if best_score >= beta_orig {
        Bound::Lower
    } else {
        Bound::Exact
    };

    ctx.cache.insert(
        key,
        CacheEntry {
            score: best_score,
            best_move,
            depth,
            bound,
        },
    );
    Some(best_score)
}

fn principal_variation(
    position: &Position,
    target: Color,
    cache: &HashMap<PositionKey, CacheEntry>,
) -> Option<Vec<Column>> {
    let mut line = Vec::new();
    let mut current = position.clone();

    loop {
        if current.winner.is_some() || current.is_draw() {
            break;
        }

        let key = PositionKey {
            board_key: current.board.canonical_key(),
            side_to_move: current.side_to_move,
        };
        let entry = cache.get(&key)?;
        let column = entry.best_move?;
        line.push(column);
        current.apply_move(column).ok()?;

        if line.len() > COLUMNS * ROWS {
            return None;
        }

        let _ = target;
    }

    Some(line)
}

fn ordered_moves(position: &Position, target: Color, tt_move: Option<Column>) -> Vec<Column> {
    let legal = position.legal_moves();
    let mut forcing = Vec::new();
    let mut safe = Vec::new();
    let mut others = Vec::new();
    let opponent = position.side_to_move.opponent();

    for column in legal {
        let mut child = position.clone();
        if child.apply_move(column).is_ok() && child.winner == Some(position.side_to_move) {
            forcing.push(column);
            continue;
        }

        let mut blocks = false;
        for opp_column in position.legal_moves() {
            let mut opp_child = position.clone();
            if opp_child.apply_move(opp_column).is_ok() && opp_child.winner == Some(opponent) && opp_column == column {
                blocks = true;
                break;
            }
        }

        if blocks || position.side_to_move == target {
            safe.push(column);
        } else {
            others.push(column);
        }
    }

    let mut ordered = Vec::new();
    if let Some(tt_move) = tt_move
        && let Some(idx) = forcing
            .iter()
            .chain(safe.iter())
            .chain(others.iter())
            .position(|&column| column == tt_move)
    {
        let mut all = forcing;
        all.extend(safe);
        all.extend(others);
        let tt = all.remove(idx);
        ordered.push(tt);
        all.sort_by_key(|column| (3_i32 - column.0 as i32).abs());
        ordered.extend(all);
        return ordered;
    }

    forcing.sort_by_key(|column| (3_i32 - column.0 as i32).abs());
    safe.sort_by_key(|column| (3_i32 - column.0 as i32).abs());
    others.sort_by_key(|column| (3_i32 - column.0 as i32).abs());
    ordered.extend(forcing);
    ordered.extend(safe);
    ordered.extend(others);
    ordered
}

fn evaluate_non_terminal(position: &Position, target: Color) -> i16 {
    let mut score = 0i16;

    for column in 0..COLUMNS {
        for row in 1..=ROWS {
            match position.board.get(crate::board::Cell::new(column, row)) {
                Some(color) if color == target => score += center_score(Column(column)),
                Some(_) => score -= center_score(Column(column)),
                None => {}
            }
        }
    }

    score + (count_immediate_wins(position, target) as i16 * 20)
        - (count_immediate_wins(position, target.opponent()) as i16 * 24)
}

fn count_immediate_wins(position: &Position, mover: Color) -> usize {
    if mover != position.side_to_move {
        let mut swapped = position.clone();
        swapped.side_to_move = mover;
        return count_immediate_wins(&swapped, mover);
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

fn center_score(column: Column) -> i16 {
    let distance = (3_i16 - column.0 as i16).abs();
    6 - distance
}

const SCORE_WIN: i16 = 10_000;
