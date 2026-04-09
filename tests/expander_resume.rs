use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use connect4_knowledge::expander::expand_book_with_paths;

fn temp_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("connect4_{name}_{nanos}.json"))
}

#[test]
fn expander_persists_frontier_and_resumes() {
    let book_path = temp_path("book");
    let frontier_path = temp_path("frontier");

    let first = expand_book_with_paths(book_path.clone(), frontier_path.clone(), 3, false, 2).unwrap();
    assert_eq!(first.processed, 3);
    assert!(first.pending_remaining > 0);
    assert!(first.checkpoints_written >= 1);

    let second = expand_book_with_paths(book_path.clone(), frontier_path.clone(), 3, false, 2).unwrap();
    assert_eq!(second.processed, 3);
    assert!(second.book_entries >= first.book_entries);

    let _ = std::fs::remove_file(book_path);
    let _ = std::fs::remove_file(frontier_path);
}
