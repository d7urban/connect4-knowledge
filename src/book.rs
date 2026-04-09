use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::board::{Color, Column, Position};
use crate::solver::Verdict;
use crate::verifier::ExactValue;

pub const DEFAULT_BOOK_PATH: &str = "book_cache.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntrySource {
    Book,
    RuleProof,
    Verifier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertOutcome {
    Inserted,
    Updated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookEntry {
    pub canonical_key: Vec<u8>,
    pub side_to_move: Color,
    pub verdict: Verdict,
    pub best_moves: Vec<Column>,
    pub source: EntrySource,
    #[serde(default)]
    pub exact_value: Option<ExactValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct BookKey {
    canonical_key: [u8; 42],
    side_to_move: Color,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BookDisk {
    entries: Vec<BookEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct Book {
    entries: HashMap<BookKey, BookEntry>,
}

impl Book {
    pub fn new() -> Self {
        let mut book = Self::default();
        book.seed_standard_opening();
        let _ = book.load_from_default_path();
        book
    }

    pub fn insert(&mut self, entry: BookEntry) -> InsertOutcome {
        let key = BookKey::from_entry(&entry);
        match self.entries.insert(key, entry) {
            Some(_) => InsertOutcome::Updated,
            None => InsertOutcome::Inserted,
        }
    }

    pub fn lookup(&self, position: &Position) -> Option<&BookEntry> {
        self.entries.get(&BookKey::from_position(position))
    }

    pub fn load_from_default_path(&mut self) -> io::Result<()> {
        self.load_from_path(default_book_path())
    }

    pub fn save_to_default_path(&self) -> io::Result<()> {
        self.save_to_path(default_book_path())
    }

    pub fn load_from_path(&mut self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(());
        }

        let contents = fs::read_to_string(path)?;
        let loaded: BookDisk = serde_json::from_str(&contents)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;

        for entry in loaded.entries {
            let _ = self.insert(entry);
        }

        Ok(())
    }

    pub fn save_to_path(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        let mut entries = self.entries.values().cloned().collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            left.canonical_key
                .cmp(&right.canonical_key)
                .then(side_ord(left.side_to_move).cmp(&side_ord(right.side_to_move)))
        });
        let contents = serde_json::to_string(&BookDisk { entries })
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        fs::write(path, contents)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> impl Iterator<Item = &BookEntry> {
        self.entries.values()
    }

    fn seed_standard_opening(&mut self) {
        let position = Position::new();
        let _ = self.insert(entry_for_position(
            &position,
            Verdict::SolvedWin,
            vec![Column(3)],
            EntrySource::Book,
        ));
    }
}

fn side_ord(color: Color) -> u8 {
    match color {
        Color::White => 0,
        Color::Black => 1,
    }
}

impl BookKey {
    fn from_entry(entry: &BookEntry) -> Self {
        let mut canonical_key = [0u8; 42];
        canonical_key.copy_from_slice(&entry.canonical_key);
        Self {
            canonical_key,
            side_to_move: entry.side_to_move,
        }
    }

    fn from_position(position: &Position) -> Self {
        Self {
            canonical_key: position.board.canonical_key(),
            side_to_move: position.side_to_move,
        }
    }
}

pub fn default_book_path() -> PathBuf {
    PathBuf::from(DEFAULT_BOOK_PATH)
}

pub fn entry_for_position(
    position: &Position,
    verdict: Verdict,
    best_moves: Vec<Column>,
    source: EntrySource,
) -> BookEntry {
    BookEntry {
        canonical_key: position.board.canonical_key().to_vec(),
        side_to_move: position.side_to_move,
        verdict: verdict.clone(),
        best_moves,
        source,
        exact_value: match verdict {
            Verdict::SolvedWin => Some(ExactValue::Win),
            Verdict::SolvedDraw | Verdict::Unresolved => None,
        },
    }
}

pub fn verifier_entry_for_position(
    position: &Position,
    exact_value: ExactValue,
    best_moves: Vec<Column>,
) -> BookEntry {
    entry_for_position(
        position,
        match exact_value {
            ExactValue::Win => Verdict::SolvedWin,
            ExactValue::Draw | ExactValue::Loss => Verdict::SolvedDraw,
        },
        best_moves,
        EntrySource::Verifier,
    )
    .with_exact_value(exact_value)
}

impl BookEntry {
    pub fn with_exact_value(mut self, exact_value: ExactValue) -> Self {
        self.exact_value = Some(exact_value);
        self
    }
}
