use connect4_knowledge::board::{Column, Position};
use connect4_knowledge::verifier::{ExactValue, SearchConfig, verify_with_config};

fn position_from_columns(columns: &[usize]) -> Position {
    let mut position = Position::new();
    for &col in columns {
        position.apply_move(Column(col)).unwrap();
    }
    position
}

#[test]
fn verifier_finds_immediate_win() {
    let position = position_from_columns(&[3, 0, 3, 0, 3, 1]);
    let result = verify_with_config(
        &position,
        position.side_to_move,
        SearchConfig {
            max_nodes: 50_000,
            max_empties: 36,
            ..SearchConfig::default()
        },
    )
    .unwrap();

    assert_eq!(result.verdict, ExactValue::Win);
    assert!(!result.witness.unwrap_or_default().is_empty());
}

#[test]
fn verifier_respects_empty_limit() {
    let position = Position::new();
    let result = verify_with_config(
        &position,
        position.side_to_move,
        SearchConfig {
            max_nodes: 50_000,
            max_empties: 8,
            ..SearchConfig::default()
        },
    );

    assert!(result.is_none());
}
