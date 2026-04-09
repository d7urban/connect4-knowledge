use connect4_knowledge::board::{Column, Position};
use connect4_knowledge::book::{EntrySource, entry_for_position};
use connect4_knowledge::facts::{EvaluationMask, analyze_position};
use connect4_knowledge::policy::choose_move;
use connect4_knowledge::rules::{RuleKind, generate_all};
use connect4_knowledge::solver::Verdict;

fn position_from_columns(columns: &[usize]) -> Position {
    let mut position = Position::new();
    for &col in columns {
        position.apply_move(Column(col)).unwrap();
    }
    position
}

#[test]
fn seeded_book_prefers_center_opening() {
    let position = Position::new();
    let root_entry = entry_for_position(
        &position,
        Verdict::SolvedWin,
        vec![Column(3)],
        EntrySource::Book,
    );
    let decision = choose_move(&position, Some(root_entry));
    assert_eq!(decision.selected_move, Some(Column(3)));
}

#[test]
fn appendix_b_a1_b1_has_before_baseinverse_and_claimeven() {
    let position = position_from_columns(&[0, 1]);
    let facts = analyze_position(&position, EvaluationMask::default());
    let solutions = generate_all(&position, &facts);

    assert!(solutions.iter().any(|s| s.rule_kind == RuleKind::Before));
    assert!(solutions.iter().any(|s| s.rule_kind == RuleKind::Baseinverse));
    assert!(solutions.iter().any(|s| s.rule_kind == RuleKind::Claimeven));
}

#[test]
fn appendix_b_c1_d1_is_rule_solved() {
    let position = position_from_columns(&[2, 3]);
    let facts = analyze_position(&position, EvaluationMask::default());
    let solutions = generate_all(&position, &facts);

    assert!(solutions.iter().any(|s| s.rule_kind == RuleKind::Before));
    assert!(solutions.iter().any(|s| s.rule_kind == RuleKind::Claimeven));
}

#[test]
fn vertical_generator_finds_odd_upper_pairs() {
    let position = position_from_columns(&[3, 4, 3, 4]);
    let facts = analyze_position(&position, EvaluationMask::default());
    let solutions = generate_all(&position, &facts);

    assert!(solutions.iter().any(|s| {
        s.rule_kind == RuleKind::Vertical
            && s.involved_squares.len() == 2
            && s.involved_squares[1].is_odd_row()
    }));
}
