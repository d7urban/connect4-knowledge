use std::fmt;
use std::path::{Path, PathBuf};

use redb::{Database, ReadableTable, ReadableTableMetadata, TableDefinition};

use crate::board::{Color, Column, Position};
use crate::book::{Book, BookEntry, EntrySource, entry_for_position};
use crate::solver::Verdict;
use crate::verifier::ExactValue;

pub const DEFAULT_BOOK_DB_PATH: &str = "book_cache.redb";

const BOOK_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("book_entries");
const CERTIFY_CACHE_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("certify_cache");
const CERTIFY_FRONTIER_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("certify_frontier");
const CERTIFY_TT_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("certify_tt");
const CERTIFY_NODE_STATE_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("certify_node_state");
const KEY_LEN: usize = 43;
const CERTIFY_KEY_LEN: usize = 44;
const CERTIFY_FRONTIER_KEY_LEN: usize = 87;

#[derive(Debug)]
pub struct BookDbError(String);

impl fmt::Display for BookDbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BookDbError {}

impl From<redb::Error> for BookDbError {
    fn from(value: redb::Error) -> Self {
        Self(value.to_string())
    }
}

impl From<redb::DatabaseError> for BookDbError {
    fn from(value: redb::DatabaseError) -> Self {
        Self(value.to_string())
    }
}

impl From<redb::TransactionError> for BookDbError {
    fn from(value: redb::TransactionError) -> Self {
        Self(value.to_string())
    }
}

impl From<redb::TableError> for BookDbError {
    fn from(value: redb::TableError) -> Self {
        Self(value.to_string())
    }
}

impl From<redb::StorageError> for BookDbError {
    fn from(value: redb::StorageError) -> Self {
        Self(value.to_string())
    }
}

impl From<redb::CommitError> for BookDbError {
    fn from(value: redb::CommitError) -> Self {
        Self(value.to_string())
    }
}

pub struct RedbBookStore {
    db: Database,
}

#[derive(Debug, Clone)]
pub struct CertifyFrontierEntry {
    pub canonical_key: [u8; 42],
    pub side_to_move: Color,
    pub empties: usize,
    pub root_child: Option<Column>,
    pub root_grandchild: Option<Column>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertifyChildStatus {
    Unknown,
    AttemptedUnresolved,
    ExactWin,
    ExactDraw,
    ExactLoss,
}

#[derive(Debug, Clone)]
pub struct CertifyChildRecord {
    pub column: Column,
    pub status: CertifyChildStatus,
}

#[derive(Debug, Clone)]
pub struct CertifyNodeState {
    pub best_move: Option<Column>,
    pub children: Vec<CertifyChildRecord>,
}

impl RedbBookStore {
    pub fn create(path: impl AsRef<Path>) -> Result<Self, BookDbError> {
        let db = Database::create(path.as_ref())?;
        let store = Self { db };
        store.ensure_schema()?;
        store.ensure_seeded()?;
        Ok(store)
    }

    pub fn open_or_create(path: impl AsRef<Path>) -> Result<Self, BookDbError> {
        let path = path.as_ref();
        if path.exists() {
            let db = Database::open(path)?;
            let store = Self { db };
            store.ensure_schema()?;
            store.ensure_seeded()?;
            Ok(store)
        } else {
            Self::create(path)
        }
    }

    pub fn insert(&self, entry: &BookEntry) -> Result<(), BookDbError> {
        let key = encode_key_from_entry(entry);
        let value = encode_entry_value(entry);

        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(BOOK_TABLE)?;
            table.insert(key.as_slice(), value.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    pub fn insert_batch(&self, entries: &[BookEntry]) -> Result<(), BookDbError> {
        if entries.is_empty() {
            return Ok(());
        }

        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(BOOK_TABLE)?;
            for entry in entries {
                let key = encode_key_from_entry(entry);
                let value = encode_entry_value(entry);
                table.insert(key.as_slice(), value.as_slice())?;
            }
        }
        write.commit()?;
        Ok(())
    }

    pub fn get(&self, position: &Position) -> Result<Option<BookEntry>, BookDbError> {
        let key = encode_key_from_position(position);
        let read = self.db.begin_read()?;
        let table = read.open_table(BOOK_TABLE)?;
        let value = table.get(key.as_slice())?;
        value
            .map(|guard| decode_entry(key, guard.value()))
            .transpose()
    }

    pub fn get_certify_cache(
        &self,
        position: &Position,
        target: Color,
    ) -> Result<Option<(ExactValue, Option<Column>)>, BookDbError> {
        let key = encode_certify_key(position, target);
        let read = self.db.begin_read()?;
        let table = read.open_table(CERTIFY_CACHE_TABLE)?;
        let value = table.get(key.as_slice())?;
        value
            .map(|guard| decode_certify_value(guard.value()))
            .transpose()
    }

    pub fn insert_certify_cache(
        &self,
        position: &Position,
        target: Color,
        exact_value: ExactValue,
        best_move: Option<Column>,
    ) -> Result<(), BookDbError> {
        let key = encode_certify_key(position, target);
        let value = encode_certify_value(exact_value, best_move);

        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(CERTIFY_CACHE_TABLE)?;
            table.insert(key.as_slice(), value.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    pub fn get_certify_tt_move(
        &self,
        position: &Position,
        target: Color,
    ) -> Result<Option<Column>, BookDbError> {
        let key = encode_certify_key(position, target);
        let read = self.db.begin_read()?;
        let table = read.open_table(CERTIFY_TT_TABLE)?;
        let value = table.get(key.as_slice())?;
        value
            .map(|guard| decode_certify_tt_value(guard.value()))
            .transpose()
    }

    pub fn insert_certify_tt_move(
        &self,
        position: &Position,
        target: Color,
        best_move: Column,
    ) -> Result<(), BookDbError> {
        let key = encode_certify_key(position, target);
        let value = [best_move.0 as u8];

        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(CERTIFY_TT_TABLE)?;
            table.insert(key.as_slice(), value.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    pub fn get_certify_node_state(
        &self,
        position: &Position,
        target: Color,
    ) -> Result<Option<CertifyNodeState>, BookDbError> {
        let key = encode_certify_key(position, target);
        let read = self.db.begin_read()?;
        let table = read.open_table(CERTIFY_NODE_STATE_TABLE)?;
        let value = table.get(key.as_slice())?;
        value
            .map(|guard| decode_certify_node_state(guard.value()))
            .transpose()
    }

    pub fn insert_certify_node_state(
        &self,
        position: &Position,
        target: Color,
        state: &CertifyNodeState,
    ) -> Result<(), BookDbError> {
        let key = encode_certify_key(position, target);
        let value = encode_certify_node_state(state);

        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(CERTIFY_NODE_STATE_TABLE)?;
            table.insert(key.as_slice(), value.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    pub fn insert_certify_frontier(
        &self,
        root: &Position,
        target: Color,
        node: &Position,
        root_child: Option<Column>,
        root_grandchild: Option<Column>,
    ) -> Result<(), BookDbError> {
        let key = encode_certify_frontier_key(root, target, node);
        let value = [
            42usize.saturating_sub(node.move_count) as u8,
            root_child.map(|column| column.0 as u8).unwrap_or(u8::MAX),
            root_grandchild.map(|column| column.0 as u8).unwrap_or(u8::MAX),
        ];

        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(CERTIFY_FRONTIER_TABLE)?;
            table.insert(key.as_slice(), value.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    pub fn remove_certify_frontier(
        &self,
        root: &Position,
        target: Color,
        node: &Position,
    ) -> Result<(), BookDbError> {
        let key = encode_certify_frontier_key(root, target, node);

        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(CERTIFY_FRONTIER_TABLE)?;
            let _ = table.remove(key.as_slice())?;
        }
        write.commit()?;
        Ok(())
    }

    pub fn list_certify_frontier(
        &self,
        root: &Position,
        target: Color,
        limit: usize,
    ) -> Result<Vec<CertifyFrontierEntry>, BookDbError> {
        let prefix = encode_certify_frontier_prefix(root, target);
        let read = self.db.begin_read()?;
        let table = read.open_table(CERTIFY_FRONTIER_TABLE)?;
        let mut entries = Vec::new();

        for item in table.iter()? {
            let (key, value) = item?;
            let key = key.value();
            if key.len() != CERTIFY_FRONTIER_KEY_LEN || !key.starts_with(prefix.as_slice()) {
                continue;
            }
            entries.push(decode_certify_frontier_entry(key, value.value())?);
        }

        entries.sort_by(|left, right| left.empties.cmp(&right.empties));
        entries.truncate(limit);
        Ok(entries)
    }

    pub fn count_certify_frontier(
        &self,
        root: &Position,
        target: Color,
    ) -> Result<usize, BookDbError> {
        let prefix = encode_certify_frontier_prefix(root, target);
        let read = self.db.begin_read()?;
        let table = read.open_table(CERTIFY_FRONTIER_TABLE)?;
        let mut count = 0usize;

        for item in table.iter()? {
            let (key, _) = item?;
            let key = key.value();
            if key.len() == CERTIFY_FRONTIER_KEY_LEN && key.starts_with(prefix.as_slice()) {
                count += 1;
            }
        }

        Ok(count)
    }

    pub fn len(&self) -> Result<usize, BookDbError> {
        let read = self.db.begin_read()?;
        let table = read.open_table(BOOK_TABLE)?;
        Ok(table.len()? as usize)
    }

    pub fn is_empty(&self) -> Result<bool, BookDbError> {
        Ok(self.len()? == 0)
    }

    pub fn import_book(&self, book: &Book) -> Result<usize, BookDbError> {
        let write = self.db.begin_write()?;
        let mut inserted = 0usize;
        {
            let mut table = write.open_table(BOOK_TABLE)?;
            for entry in book.entries() {
                let key = encode_key_from_entry(entry);
                let value = encode_entry_value(entry);
                table.insert(key.as_slice(), value.as_slice())?;
                inserted += 1;
            }
        }
        write.commit()?;
        Ok(inserted)
    }

    pub fn all_entries(&self) -> Result<Vec<BookEntry>, BookDbError> {
        let read = self.db.begin_read()?;
        let table = read.open_table(BOOK_TABLE)?;
        let iter = table.iter()?;
        let mut entries = Vec::with_capacity(table.len()? as usize);

        for item in iter {
            let (key, value) = item?;
            let key = key.value();
            if key.len() != KEY_LEN {
                return Err(BookDbError("bookdb key length mismatch".to_string()));
            }
            let mut key_buf = [0u8; KEY_LEN];
            key_buf.copy_from_slice(key);
            entries.push(decode_entry(key_buf, value.value())?);
        }

        Ok(entries)
    }

    fn ensure_seeded(&self) -> Result<(), BookDbError> {
        if !self.is_empty()? {
            return Ok(());
        }
        let root = Position::new();
        self.insert(&entry_for_position(
            &root,
            Verdict::SolvedWin,
            vec![Column(3)],
            EntrySource::Book,
        ))
    }

    fn ensure_schema(&self) -> Result<(), BookDbError> {
        let write = self.db.begin_write()?;
        {
            let _ = write.open_table(BOOK_TABLE)?;
            let _ = write.open_table(CERTIFY_CACHE_TABLE)?;
            let _ = write.open_table(CERTIFY_FRONTIER_TABLE)?;
            let _ = write.open_table(CERTIFY_TT_TABLE)?;
            let _ = write.open_table(CERTIFY_NODE_STATE_TABLE)?;
        }
        write.commit()?;
        Ok(())
    }
}

pub fn default_book_db_path() -> PathBuf {
    PathBuf::from(DEFAULT_BOOK_DB_PATH)
}

fn encode_key_from_position(position: &Position) -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    key[..42].copy_from_slice(&position.board.canonical_key());
    key[42] = encode_color(position.side_to_move);
    key
}

fn encode_key_from_entry(entry: &BookEntry) -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    key[..42].copy_from_slice(&entry.canonical_key);
    key[42] = encode_color(entry.side_to_move);
    key
}

fn encode_certify_key(position: &Position, target: Color) -> [u8; CERTIFY_KEY_LEN] {
    let mut key = [0u8; CERTIFY_KEY_LEN];
    key[..42].copy_from_slice(&position.board.canonical_key());
    key[42] = encode_color(position.side_to_move);
    key[43] = encode_color(target);
    key
}

fn encode_certify_frontier_prefix(root: &Position, target: Color) -> [u8; CERTIFY_KEY_LEN] {
    encode_certify_key(root, target)
}

fn encode_certify_frontier_key(
    root: &Position,
    target: Color,
    node: &Position,
) -> [u8; CERTIFY_FRONTIER_KEY_LEN] {
    let mut key = [0u8; CERTIFY_FRONTIER_KEY_LEN];
    key[..CERTIFY_KEY_LEN].copy_from_slice(&encode_certify_key(root, target));
    key[CERTIFY_KEY_LEN..(CERTIFY_KEY_LEN + 42)].copy_from_slice(&node.board.canonical_key());
    key[CERTIFY_KEY_LEN + 42] = encode_color(node.side_to_move);
    key
}

fn encode_entry_value(entry: &BookEntry) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(4 + entry.best_moves.len());
    bytes.push(encode_verdict(entry.verdict.clone()));
    bytes.push(encode_source(entry.source.clone()));
    bytes.push(entry.best_moves.len() as u8);
    bytes.extend(entry.best_moves.iter().map(|column| column.0 as u8));
    if let Some(exact_value) = entry.exact_value {
        bytes.push(encode_exact_value(exact_value));
    }
    bytes
}

fn encode_certify_value(exact_value: ExactValue, best_move: Option<Column>) -> Vec<u8> {
    vec![
        encode_exact_value(exact_value),
        best_move.map(|column| column.0 as u8).unwrap_or(u8::MAX),
    ]
}

fn decode_entry(key: [u8; KEY_LEN], value: &[u8]) -> Result<BookEntry, BookDbError> {
    if value.len() < 3 {
        return Err(BookDbError("bookdb value too short".to_string()));
    }
    let move_count = value[2] as usize;
    if value.len() != 3 + move_count && value.len() != 4 + move_count {
        return Err(BookDbError("bookdb move payload length mismatch".to_string()));
    }
    let exact_value = if value.len() == 4 + move_count {
        Some(decode_exact_value(value[3 + move_count])?)
    } else {
        None
    };

    Ok(BookEntry {
        canonical_key: key[..42].to_vec(),
        side_to_move: decode_color(key[42])?,
        verdict: decode_verdict(value[0])?,
        source: decode_source(value[1])?,
        best_moves: value[3..(3 + move_count)]
            .iter()
            .map(|&column| Column(column as usize))
            .collect(),
        exact_value,
    })
}

fn decode_certify_value(value: &[u8]) -> Result<(ExactValue, Option<Column>), BookDbError> {
    if value.len() != 2 {
        return Err(BookDbError("certify cache value length mismatch".to_string()));
    }

    let exact_value = decode_exact_value(value[0])?;
    let best_move = if value[1] == u8::MAX {
        None
    } else {
        Some(Column(value[1] as usize))
    };
    Ok((exact_value, best_move))
}

fn decode_certify_tt_value(value: &[u8]) -> Result<Column, BookDbError> {
    if value.len() != 1 {
        return Err(BookDbError("certify tt value length mismatch".to_string()));
    }
    Ok(Column(value[0] as usize))
}

fn encode_certify_node_state(state: &CertifyNodeState) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(2 + state.children.len() * 2);
    bytes.push(state.best_move.map(|column| column.0 as u8).unwrap_or(u8::MAX));
    bytes.push(state.children.len() as u8);
    for child in &state.children {
        bytes.push(child.column.0 as u8);
        bytes.push(encode_certify_child_status(child.status));
    }
    bytes
}

fn decode_certify_node_state(value: &[u8]) -> Result<CertifyNodeState, BookDbError> {
    if value.len() < 2 {
        return Err(BookDbError("certify node state too short".to_string()));
    }
    let child_count = value[1] as usize;
    if value.len() != 2 + child_count * 2 {
        return Err(BookDbError("certify node state length mismatch".to_string()));
    }
    let mut children = Vec::with_capacity(child_count);
    for idx in 0..child_count {
        let offset = 2 + idx * 2;
        children.push(CertifyChildRecord {
            column: Column(value[offset] as usize),
            status: decode_certify_child_status(value[offset + 1])?,
        });
    }
    Ok(CertifyNodeState {
        best_move: (value[0] != u8::MAX).then_some(Column(value[0] as usize)),
        children,
    })
}

fn encode_certify_child_status(status: CertifyChildStatus) -> u8 {
    match status {
        CertifyChildStatus::Unknown => 0,
        CertifyChildStatus::AttemptedUnresolved => 1,
        CertifyChildStatus::ExactWin => 2,
        CertifyChildStatus::ExactDraw => 3,
        CertifyChildStatus::ExactLoss => 4,
    }
}

fn decode_certify_child_status(value: u8) -> Result<CertifyChildStatus, BookDbError> {
    match value {
        0 => Ok(CertifyChildStatus::Unknown),
        1 => Ok(CertifyChildStatus::AttemptedUnresolved),
        2 => Ok(CertifyChildStatus::ExactWin),
        3 => Ok(CertifyChildStatus::ExactDraw),
        4 => Ok(CertifyChildStatus::ExactLoss),
        _ => Err(BookDbError("invalid certify child status".to_string())),
    }
}

fn decode_certify_frontier_entry(
    key: &[u8],
    value: &[u8],
) -> Result<CertifyFrontierEntry, BookDbError> {
    if key.len() != CERTIFY_FRONTIER_KEY_LEN {
        return Err(BookDbError("certify frontier key length mismatch".to_string()));
    }
    if value.len() != 1 && value.len() != 2 && value.len() != 3 {
        return Err(BookDbError("certify frontier value length mismatch".to_string()));
    }

    let mut canonical_key = [0u8; 42];
    canonical_key.copy_from_slice(&key[CERTIFY_KEY_LEN..(CERTIFY_KEY_LEN + 42)]);
    Ok(CertifyFrontierEntry {
        canonical_key,
        side_to_move: decode_color(key[CERTIFY_KEY_LEN + 42])?,
        empties: value[0] as usize,
        root_child: if value.len() >= 2 && value[1] != u8::MAX {
            Some(Column(value[1] as usize))
        } else {
            None
        },
        root_grandchild: if value.len() == 3 && value[2] != u8::MAX {
            Some(Column(value[2] as usize))
        } else {
            None
        },
    })
}

fn encode_color(color: Color) -> u8 {
    match color {
        Color::White => 0,
        Color::Black => 1,
    }
}

fn decode_color(value: u8) -> Result<Color, BookDbError> {
    match value {
        0 => Ok(Color::White),
        1 => Ok(Color::Black),
        _ => Err(BookDbError("invalid color tag".to_string())),
    }
}

fn encode_verdict(verdict: Verdict) -> u8 {
    match verdict {
        Verdict::SolvedWin => 0,
        Verdict::SolvedDraw => 1,
        Verdict::Unresolved => 2,
    }
}

fn decode_verdict(value: u8) -> Result<Verdict, BookDbError> {
    match value {
        0 => Ok(Verdict::SolvedWin),
        1 => Ok(Verdict::SolvedDraw),
        2 => Ok(Verdict::Unresolved),
        _ => Err(BookDbError("invalid verdict tag".to_string())),
    }
}

fn encode_source(source: EntrySource) -> u8 {
    match source {
        EntrySource::Book => 0,
        EntrySource::RuleProof => 1,
        EntrySource::Verifier => 2,
    }
}

fn decode_source(value: u8) -> Result<EntrySource, BookDbError> {
    match value {
        0 => Ok(EntrySource::Book),
        1 => Ok(EntrySource::RuleProof),
        2 => Ok(EntrySource::Verifier),
        _ => Err(BookDbError("invalid source tag".to_string())),
    }
}

fn encode_exact_value(value: ExactValue) -> u8 {
    match value {
        ExactValue::Win => 0,
        ExactValue::Draw => 1,
        ExactValue::Loss => 2,
    }
}

fn decode_exact_value(value: u8) -> Result<ExactValue, BookDbError> {
    match value {
        0 => Ok(ExactValue::Win),
        1 => Ok(ExactValue::Draw),
        2 => Ok(ExactValue::Loss),
        _ => Err(BookDbError("invalid exact value tag".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{BookDbError, RedbBookStore};
    use crate::board::{Column, Position};
    use crate::book::{EntrySource, entry_for_position};
    use crate::solver::Verdict;

    #[test]
    fn redb_store_round_trips_entry() -> Result<(), BookDbError> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("connect4_bookdb_{nanos}.redb"));
        let store = RedbBookStore::create(&path)?;

        let mut position = Position::new();
        position.apply_move(Column(3)).expect("move");
        let entry = entry_for_position(
            &position,
            Verdict::SolvedDraw,
            vec![Column(2), Column(4)],
            EntrySource::Verifier,
        );

        store.insert(&entry)?;
        let loaded = store.get(&position)?.expect("stored entry");
        assert_eq!(loaded, entry);

        let _ = std::fs::remove_file(path);
        Ok(())
    }
}
