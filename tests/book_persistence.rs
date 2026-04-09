use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use connect4_knowledge::board::{Column, Position};
use connect4_knowledge::book::{Book, EntrySource, entry_for_position};
use connect4_knowledge::solver::Verdict;

fn temp_path() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("connect4_book_test_{nanos}.json"))
}

#[test]
fn book_round_trips_through_json() {
    let path = temp_path();
    let mut book = Book::new();

    let mut position = Position::new();
    position.apply_move(Column(3)).unwrap();

    book.insert(entry_for_position(
        &position,
        Verdict::SolvedDraw,
        vec![Column(2)],
        EntrySource::Verifier,
    ));
    book.save_to_path(&path).unwrap();

    let mut loaded = Book::default();
    loaded.load_from_path(&path).unwrap();

    let loaded_entry = loaded.lookup(&position).unwrap();
    assert_eq!(loaded_entry.verdict, Verdict::SolvedDraw);
    assert_eq!(loaded_entry.best_moves, vec![Column(2)]);

    let _ = std::fs::remove_file(path);
}
