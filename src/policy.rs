use crate::board::{Column, Position};
use crate::book::{BookEntry, verifier_entry_for_position};
use crate::facts::{EvaluationMask, analyze_position};
use crate::groups::{evaluate_groups, standard_groups};
use crate::rules::generate_all;
use crate::solver::{Proof, Verdict, solve_cover};
use crate::verifier::{ExactValue, SearchConfig, VerifierResult, search_best_move, verify_with_config};

const MAX_RULE_PROBLEMS_FOR_SEARCH: usize = 18;
const MAX_RULE_SOLUTIONS_FOR_SEARCH: usize = 160;

#[derive(Debug, Clone)]
pub enum DecisionBasis {
    ImmediateTactic,
    RuleProof,
    Verifier,
    Search,
    KnowledgeHeuristic,
    Book,
    Fallback,
}

impl DecisionBasis {
    pub fn label(&self) -> &'static str {
        match self {
            Self::ImmediateTactic => "ImmediateTactic",
            Self::RuleProof => "RuleProof",
            Self::Verifier => "Verifier",
            Self::Search => "Search",
            Self::KnowledgeHeuristic => "Heuristic",
            Self::Book => "Book",
            Self::Fallback => "Fallback",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Decision {
    pub selected_move: Option<Column>,
    pub basis: DecisionBasis,
    pub proof: Option<Proof>,
    pub explanation: String,
    pub certified_entries: Vec<BookEntry>,
}

pub fn choose_move(position: &Position, book_entry: Option<BookEntry>) -> Decision {
    if let Some(entry) = book_entry {
        return Decision {
            selected_move: entry.best_moves.first().copied(),
            basis: DecisionBasis::Book,
            proof: None,
            explanation: "selected move from certified book".to_string(),
            certified_entries: Vec::new(),
        };
    }

    let legal_moves = position.legal_moves();
    if legal_moves.is_empty() {
        return Decision {
            selected_move: None,
            basis: DecisionBasis::Fallback,
            proof: None,
            explanation: "no legal moves available".to_string(),
            certified_entries: Vec::new(),
        };
    }

    let side = position.side_to_move;
    let immediate_wins = winning_moves(position, side);
    if let Some(column) = best_centered(immediate_wins) {
        return Decision {
            selected_move: Some(column),
            basis: DecisionBasis::ImmediateTactic,
            proof: None,
            explanation: format!("play {} to win immediately", column_name(column)),
            certified_entries: Vec::new(),
        };
    }

    let opponent = side.opponent();
    let opponent_wins = winning_moves(position, opponent);
    if let Some(column) = best_centered(shared_legal_moves(&legal_moves, &opponent_wins)) {
        return Decision {
            selected_move: Some(column),
            basis: DecisionBasis::ImmediateTactic,
            proof: None,
            explanation: format!("play {} to block the opponent's immediate win", column_name(column)),
            certified_entries: Vec::new(),
        };
    }

    if let Some(search) = search_best_move(position, side, 8, 400_000) {
        return Decision {
            selected_move: Some(search.best_move),
            basis: DecisionBasis::Search,
            proof: None,
            explanation: format!(
                "play {} from bounded alpha-beta search (depth {}, score {}, nodes {})",
                column_name(search.best_move),
                search.depth_reached,
                search.score,
                search.nodes_visited
            ),
            certified_entries: Vec::new(),
        };
    }

    let analyses: Vec<_> = legal_moves
        .iter()
        .copied()
        .filter_map(|column| analyze_candidate(position, column, side))
        .collect();
    let certified_entries = collect_certified_entries(position, &analyses);

    if let Some(best) = analyses
        .iter()
        .filter(|analysis| analysis.proof_verdict == Some(Verdict::SolvedWin))
        .max_by_key(|analysis| (analysis.score, center_weight(analysis.column)))
    {
        return Decision {
            selected_move: Some(best.column),
            basis: DecisionBasis::RuleProof,
            proof: best.proof.clone(),
            explanation: format!("play {} because the child position has a winning rule proof", column_name(best.column)),
            certified_entries,
        };
    }

    if let Some(best) = analyses
        .iter()
        .filter(|analysis| analysis.proof_verdict == Some(Verdict::SolvedDraw))
        .filter(|analysis| analysis.opponent_immediate_wins.is_empty())
        .max_by_key(|analysis| (analysis.score, center_weight(analysis.column)))
        .or_else(|| {
            analyses
                .iter()
                .filter(|analysis| analysis.proof_verdict == Some(Verdict::SolvedDraw))
                .max_by_key(|analysis| (analysis.score, center_weight(analysis.column)))
        })
    {
        return Decision {
            selected_move: Some(best.column),
            basis: DecisionBasis::RuleProof,
            proof: best.proof.clone(),
            explanation: format!("play {} because the child position has a drawing rule proof", column_name(best.column)),
            certified_entries,
        };
    }

    if let Some(best) = analyses
        .iter()
        .filter(|analysis| analysis.verifier_value == Some(ExactValue::Win))
        .max_by_key(|analysis| (analysis.score, center_weight(analysis.column)))
    {
        return Decision {
            selected_move: Some(best.column),
            basis: DecisionBasis::Verifier,
            proof: best.proof.clone(),
            explanation: format!(
                "play {} because the exact verifier finds a win for this line",
                column_name(best.column)
            ),
            certified_entries,
        };
    }

    if let Some(best) = analyses
        .iter()
        .filter(|analysis| analysis.verifier_value == Some(ExactValue::Draw))
        .filter(|analysis| analysis.opponent_immediate_wins.is_empty())
        .max_by_key(|analysis| (analysis.score, center_weight(analysis.column)))
        .or_else(|| {
            analyses
                .iter()
                .filter(|analysis| analysis.verifier_value == Some(ExactValue::Draw))
                .max_by_key(|analysis| (analysis.score, center_weight(analysis.column)))
        })
    {
        return Decision {
            selected_move: Some(best.column),
            basis: DecisionBasis::Verifier,
            proof: best.proof.clone(),
            explanation: format!(
                "play {} because the exact verifier preserves a draw",
                column_name(best.column)
            ),
            certified_entries,
        };
    }

    if let Some(best) = analyses
        .iter()
        .filter(|analysis| analysis.opponent_immediate_wins.is_empty())
        .max_by_key(|analysis| analysis.score)
        .or_else(|| analyses.iter().max_by_key(|analysis| analysis.score))
    {
        return Decision {
            selected_move: Some(best.column),
            basis: DecisionBasis::KnowledgeHeuristic,
            proof: None,
            explanation: best.explanation(),
            certified_entries,
        };
    }

    Decision {
        selected_move: legal_moves.first().copied(),
        basis: DecisionBasis::Fallback,
        proof: None,
        explanation: "falling back to the first legal move".to_string(),
        certified_entries,
    }
}

#[derive(Debug, Clone)]
struct CandidateAnalysis {
    column: Column,
    child_position: Position,
    score: i32,
    proof_verdict: Option<Verdict>,
    proof: Option<Proof>,
    verifier_value: Option<ExactValue>,
    verifier_result: Option<VerifierResult>,
    our_immediate_wins_next: usize,
    opponent_immediate_wins: Vec<Column>,
    playable_threats: usize,
}

impl CandidateAnalysis {
    fn explanation(&self) -> String {
        if self.opponent_immediate_wins.is_empty() {
            format!(
                "play {} for the best threat and position score (threats: {}, follow-up wins: {})",
                column_name(self.column),
                self.playable_threats,
                self.our_immediate_wins_next
            )
        } else {
            format!(
                "play {} despite danger; opponent threats remain in {} column(s)",
                column_name(self.column),
                self.opponent_immediate_wins.len()
            )
        }
    }
}

fn analyze_candidate(position: &Position, column: Column, root_side: crate::board::Color) -> Option<CandidateAnalysis> {
    let mut child = position.clone();
    child.apply_move(column).ok()?;

    let side = position.side_to_move;
    let opponent_immediate_wins = winning_moves(&child, child.side_to_move);
    let our_immediate_wins_next = count_next_turn_wins(&child, side);
    let playable_threats = count_playable_threats(&child, side);
    let centrality = center_weight(column);
    let structural = score_position(&child, side);
    let facts = analyze_position(&child, EvaluationMask::default());
    let solutions = generate_all(&child, &facts);
    let proof = bounded_rule_proof(&facts, &solutions);
    let proof_verdict = Some(proof.verdict.clone());
    let verifier_result = verify_with_config(
        &child,
        root_side,
        SearchConfig {
            max_nodes: 200_000,
            ..SearchConfig::default()
        },
    );
    let verifier_value = verifier_result.as_ref().map(|result| result.verdict);

    let score = structural
        + centrality
        + (our_immediate_wins_next as i32 * 180)
        + (playable_threats as i32 * 70)
        + proof_bonus(&proof)
        + verifier_bonus(verifier_value)
        - (opponent_immediate_wins.len() as i32 * 500);

    Some(CandidateAnalysis {
        column,
        child_position: child,
        score,
        proof_verdict,
        proof: Some(proof),
        verifier_value,
        verifier_result,
        our_immediate_wins_next,
        opponent_immediate_wins,
        playable_threats,
    })
}

fn winning_moves(position: &Position, mover: crate::board::Color) -> Vec<Column> {
    if mover != position.side_to_move {
        let mut swapped = position.clone();
        swapped.side_to_move = mover;
        return winning_moves(&swapped, mover);
    }

    position
        .legal_moves()
        .into_iter()
        .filter(|&column| {
            let mut child = position.clone();
            child.apply_move(column).ok();
            child.winner == Some(mover)
        })
        .collect()
}

fn count_next_turn_wins(position: &Position, perspective: crate::board::Color) -> usize {
    position
        .legal_moves()
        .into_iter()
        .filter(|&opponent_column| {
            let mut reply_position = position.clone();
            reply_position.apply_move(opponent_column).ok();
            !winning_moves(&reply_position, perspective).is_empty()
        })
        .count()
}

fn count_playable_threats(position: &Position, perspective: crate::board::Color) -> usize {
    let groups = standard_groups();
    let states = evaluate_groups(&position.board, &groups);

    groups
        .iter()
        .zip(states.iter())
        .filter(|(group, state)| {
            let live = match perspective {
                crate::board::Color::White => state.live_for_white,
                crate::board::Color::Black => state.live_for_black,
            };

            live && state.empty_count == 1 && group.cells.iter().any(|&cell| is_playable(position, cell))
        })
        .count()
}

fn score_position(position: &Position, perspective: crate::board::Color) -> i32 {
    let groups = standard_groups();
    let states = evaluate_groups(&position.board, &groups);

    let mut score = 0i32;

    for col in 0..crate::board::COLUMNS {
        for row in 1..=crate::board::ROWS {
            if position.board.get(crate::board::Cell::new(col, row)) == Some(perspective) {
                score += center_weight(Column(col));
            }
        }
    }

    for state in states {
        if state.live_for_white && perspective == crate::board::Color::White {
            score += group_value(state.white_count, state.empty_count);
        }
        if state.live_for_black && perspective == crate::board::Color::Black {
            score += group_value(state.black_count, state.empty_count);
        }
        if state.live_for_white && perspective == crate::board::Color::Black {
            score -= group_value(state.white_count, state.empty_count);
        }
        if state.live_for_black && perspective == crate::board::Color::White {
            score -= group_value(state.black_count, state.empty_count);
        }
    }

    score
}

fn group_value(occupied: usize, empty: usize) -> i32 {
    match (occupied, empty) {
        (4, 0) => 20_000,
        (3, 1) => 900,
        (2, 2) => 120,
        (1, 3) => 20,
        _ => 0,
    }
}

fn proof_bonus(proof: &Proof) -> i32 {
    match proof.verdict {
        Verdict::SolvedWin => 5_000,
        Verdict::SolvedDraw => 1_200,
        Verdict::Unresolved => 0,
    }
}

fn verifier_bonus(value: Option<ExactValue>) -> i32 {
    match value {
        Some(ExactValue::Win) => 4_000,
        Some(ExactValue::Draw) => 900,
        Some(ExactValue::Loss) => -4_000,
        None => 0,
    }
}

fn collect_certified_entries(position: &Position, analyses: &[CandidateAnalysis]) -> Vec<BookEntry> {
    let mut entries = Vec::new();

    for analysis in analyses {
        if let Some(result) = &analysis.verifier_result {
            let best_moves = result
                .witness
                .as_ref()
                .and_then(|line| line.first().copied())
                .map(|column| vec![column])
                .unwrap_or_default();
            entries.push(verifier_entry_for_position(
                &analysis.child_position,
                result.verdict,
                best_moves,
            ));
        }
    }

    if let Some(best) = analyses
        .iter()
        .find(|analysis| analysis.verifier_value == Some(ExactValue::Win))
    {
        entries.push(verifier_entry_for_position(
            position,
            ExactValue::Win,
            vec![best.column],
        ));
    } else if let Some(best) = analyses
        .iter()
        .find(|analysis| analysis.verifier_value == Some(ExactValue::Draw))
    {
        entries.push(verifier_entry_for_position(
            position,
            ExactValue::Draw,
            vec![best.column],
        ));
    }

    entries
}

fn bounded_rule_proof(
    facts: &crate::facts::PositionFacts,
    solutions: &[crate::rules::Solution],
) -> Proof {
    if facts.problems.len() > MAX_RULE_PROBLEMS_FOR_SEARCH
        || solutions.len() > MAX_RULE_SOLUTIONS_FOR_SEARCH
    {
        return Proof {
            verdict: Verdict::Unresolved,
            chosen_solution_ids: Vec::new(),
            unresolved_problem_ids: facts.problems.iter().map(|problem| problem.group_id).collect(),
        };
    }

    solve_cover(&facts.problems, solutions)
}

fn is_playable(position: &Position, cell: crate::board::Cell) -> bool {
    position
        .board
        .playable_cell(Column(cell.col))
        .is_ok_and(|playable| playable.row == cell.row)
}

fn center_weight(column: Column) -> i32 {
    let distance = (3_i32 - column.0 as i32).abs();
    10 - distance * 2
}

fn column_name(column: Column) -> char {
    (b'a' + column.0 as u8) as char
}

fn best_centered(columns: Vec<Column>) -> Option<Column> {
    columns.into_iter().max_by_key(|&column| center_weight(column))
}

fn shared_legal_moves(legal_moves: &[Column], target_moves: &[Column]) -> Vec<Column> {
    legal_moves
        .iter()
        .copied()
        .filter(|column| target_moves.contains(column))
        .collect()
}
