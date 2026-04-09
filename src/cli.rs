use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::board::{Column, Position};
use crate::book::{Book, EntrySource};
use crate::bookdb::{RedbBookStore, default_book_db_path};
use crate::expander::{
    certify_position_with_options, default_checkpoint_every, expand_book_with_options,
    expand_branch_with_options,
    inspect_certify_root_state,
};
use crate::facts::{EvaluationMask, analyze_position};
use crate::gui::run_gui;
use crate::policy::choose_move;
use crate::rules::generate_all;
use crate::solver::{Verdict, solve_cover};

pub fn run<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    let mut iter = args.into_iter();
    let Some(command) = iter.next() else {
        return run_gui().map_err(|err| err.to_string());
    };

    match command.as_str() {
        "help" | "--help" | "-h" => {
            print_usage();
            Ok(())
        }
        "gui" => run_gui().map_err(|err| err.to_string()),
        "expand-book" => cmd_expand_book(iter),
        "expand-branch" => cmd_expand_branch(iter),
        "certify-position" => cmd_certify_position(iter),
        "play" => cmd_play(iter),
        "analyze" => cmd_analyze(iter),
        "explain" => cmd_explain(iter),
        "dump-proof" => cmd_dump_proof(iter),
        "report-book" => cmd_report_book(iter),
        "migrate-book-db" => cmd_migrate_book_db(),
        _ => Err(format!("unknown command: {command}")),
    }
}

fn cmd_play<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    let position = position_from_moves(args)?;
    let store = RedbBookStore::open_or_create(default_book_db_path())
        .map_err(|err| format!("failed to open book db: {err}"))?;
    let decision = choose_move(
        &position,
        store.get(&position).map_err(|err| format!("failed to query book db: {err}"))?,
    );
    println!("{decision:#?}");
    Ok(())
}

fn cmd_analyze<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    let position = position_from_moves(args)?;
    let facts = analyze_position(&position, EvaluationMask::default());
    let solutions = generate_all(&position, &facts);

    println!("side_to_move: {:?}", position.side_to_move);
    println!("problems: {}", facts.problems.len());
    println!("solutions: {}", solutions.len());
    Ok(())
}

fn cmd_explain<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    let position = position_from_moves(args)?;
    let store = RedbBookStore::open_or_create(default_book_db_path())
        .map_err(|err| format!("failed to open book db: {err}"))?;
    let decision = choose_move(
        &position,
        store.get(&position).map_err(|err| format!("failed to query book db: {err}"))?,
    );
    println!("basis: {}", decision.basis.label());
    if let Some(column) = decision.selected_move {
        println!("move: {}", (b'a' + column.0 as u8) as char);
    } else {
        println!("move: none");
    }
    println!("explanation: {}", decision.explanation);
    Ok(())
}

fn cmd_dump_proof<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    let position = position_from_moves(args)?;
    let facts = analyze_position(&position, EvaluationMask::default());
    let solutions = generate_all(&position, &facts);
    let proof = solve_cover(&facts.problems, &solutions);
    println!("{proof:#?}");
    Ok(())
}

fn cmd_expand_book<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    let mut batch_limit = 200usize;
    let mut reset_frontier = false;
    let mut checkpoint_every = default_checkpoint_every();
    let mut iter = args.into_iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--reset-frontier" => reset_frontier = true,
            "--checkpoint-every" => {
                let Some(value) = iter.next() else {
                    return Err("missing value for --checkpoint-every".to_string());
                };
                checkpoint_every = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid checkpoint interval `{value}`"))?;
            }
            _ => {
                batch_limit = arg
                    .parse::<usize>()
                    .map_err(|_| format!("invalid argument `{arg}`"))?;
            }
        }
    }

    let report = expand_book_with_options(batch_limit, reset_frontier, checkpoint_every)?;
    println!("processed: {}", report.processed);
    println!("enqueued: {}", report.enqueued);
    println!("verifier_solved: {}", report.verifier_solved);
    println!("book_inserted: {}", report.book_inserted);
    println!("book_updated: {}", report.book_updated);
    println!("verifier_attempted: {}", report.verifier_attempted);
    println!("verifier_skipped: {}", report.verifier_skipped);
    println!("verifier_cached: {}", report.verifier_cached);
    println!("expansion_pruned: {}", report.expansion_pruned);
    println!("frontier_reseeded: {}", report.frontier_reseeded);
    println!("checkpoints_written: {}", report.checkpoints_written);
    println!("pending_remaining: {}", report.pending_remaining);
    println!("book_entries: {}", report.book_entries);
    Ok(())
}

fn cmd_expand_branch<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    let mut batch_limit = 200usize;
    let mut reset_frontier = false;
    let mut checkpoint_every = default_checkpoint_every();
    let mut moves = Vec::new();
    let mut iter = args.into_iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--reset-frontier" => reset_frontier = true,
            "--checkpoint-every" => {
                let Some(value) = iter.next() else {
                    return Err("missing value for --checkpoint-every".to_string());
                };
                checkpoint_every = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid checkpoint interval `{value}`"))?;
            }
            "--batch-limit" => {
                let Some(value) = iter.next() else {
                    return Err("missing value for --batch-limit".to_string());
                };
                batch_limit = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid batch limit `{value}`"))?;
            }
            _ if arg.starts_with("--") => return Err(format!("invalid argument `{arg}`")),
            _ if arg.chars().all(|ch| matches!(ch, 'a'..='g' | '1'..='7')) && arg.len() > 1 => {
                for ch in arg.chars() {
                    moves.push(parse_column(&ch.to_string())?);
                }
            }
            _ => moves.push(parse_column(&arg)?),
        }
    }

    if moves.is_empty() {
        return Err("expand-branch requires at least one move, for example `expand-branch d d`".to_string());
    }

    let (report, frontier_path) =
        expand_branch_with_options(&moves, batch_limit, reset_frontier, checkpoint_every)?;
    println!("branch: {}", moves.iter().map(|column| (b'a' + column.0 as u8) as char).collect::<String>());
    println!("frontier_path: {}", frontier_path.display());
    println!("processed: {}", report.processed);
    println!("enqueued: {}", report.enqueued);
    println!("verifier_solved: {}", report.verifier_solved);
    println!("book_inserted: {}", report.book_inserted);
    println!("book_updated: {}", report.book_updated);
    println!("verifier_attempted: {}", report.verifier_attempted);
    println!("verifier_skipped: {}", report.verifier_skipped);
    println!("verifier_cached: {}", report.verifier_cached);
    println!("expansion_pruned: {}", report.expansion_pruned);
    println!("frontier_reseeded: {}", report.frontier_reseeded);
    println!("checkpoints_written: {}", report.checkpoints_written);
    println!("pending_remaining: {}", report.pending_remaining);
    println!("book_entries: {}", report.book_entries);
    Ok(())
}

fn cmd_certify_position<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    let mut max_nodes = 2_000_000usize;
    let mut auto_double = false;
    let mut dump_root_state = false;
    let mut follow_blockers = false;
    let mut follow_steps: Option<usize> = None;
    let mut follow_until_stall = false;
    let mut stall_limit = 3usize;
    let mut total_max_nodes: Option<usize> = None;
    let mut moves = Vec::new();
    let mut iter = args.into_iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--auto-double" => auto_double = true,
            "--dump-root-state" => dump_root_state = true,
            "--follow-blockers" => follow_blockers = true,
            "--follow-until-stall" => {
                follow_blockers = true;
                follow_until_stall = true;
            }
            "--follow-steps" => {
                let Some(value) = iter.next() else {
                    return Err("missing value for --follow-steps".to_string());
                };
                follow_steps = Some(
                    value
                        .parse::<usize>()
                        .map_err(|_| format!("invalid follow step count `{value}`"))?,
                );
            }
            "--stall-limit" => {
                let Some(value) = iter.next() else {
                    return Err("missing value for --stall-limit".to_string());
                };
                stall_limit = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid stall limit `{value}`"))?;
            }
            "--total-max-nodes" => {
                let Some(value) = iter.next() else {
                    return Err("missing value for --total-max-nodes".to_string());
                };
                total_max_nodes = Some(
                    value
                        .parse::<usize>()
                        .map_err(|_| format!("invalid total max node count `{value}`"))?,
                );
            }
            "--max-nodes" => {
                let Some(value) = iter.next() else {
                    return Err("missing value for --max-nodes".to_string());
                };
                max_nodes = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid max node count `{value}`"))?;
            }
            _ if arg.starts_with("--") => return Err(format!("invalid argument `{arg}`")),
            _ if arg.chars().all(|ch| matches!(ch, 'a'..='g' | '1'..='7')) && arg.len() > 1 => {
                for ch in arg.chars() {
                    moves.push(parse_column(&ch.to_string())?);
                }
            }
            _ => moves.push(parse_column(&arg)?),
        }
    }

    if dump_root_state {
        let report = inspect_certify_root_state(&moves)?;
        print_root_state(&report);
        return Ok(());
    }

    let mut attempt = 1usize;
    let mut followed = 0usize;
    let mut total_nodes_visited = 0usize;
    let mut stall_count = 0usize;
    let mut last_progress_signature: Option<(usize, usize, usize)> = None;

    loop {
        if auto_double {
            println!("attempt: {attempt}");
        }
        let position_label = format_moves(&moves);
        let report = certify_position_with_options(&moves, max_nodes)?;
        println!("position: {position_label}");
        println!("max_nodes: {max_nodes}");
        println!("certified: {}", report.certified);
        if let Some(exact_value) = report.exact_value {
            println!("exact_value: {exact_value:?}");
        }
        if let Some(best_move) = report.best_move {
            println!("best_move: {}", (b'a' + best_move.0 as u8) as char);
        }
        println!("inserted: {}", report.inserted);
        println!("updated: {}", report.updated);
        println!("nodes_visited: {}", report.nodes_visited);
        println!("book_hits: {}", report.book_hits);
        println!("certify_cache_hits: {}", report.certify_cache_hits);
        println!("certify_tt_hits: {}", report.certify_tt_hits);
        println!("certify_frontier_resumed: {}", report.certify_frontier_resumed);
        println!("certify_frontier_solved: {}", report.certify_frontier_solved);
        println!("certify_frontier_requeued: {}", report.certify_frontier_requeued);
        println!("certify_frontier_remaining: {}", report.certify_frontier_remaining);
        if let Some(min_empties) = report.certify_frontier_min_empties {
            println!("certify_frontier_min_empties: {min_empties}");
        }
        if let Some(max_empties) = report.certify_frontier_max_empties {
            println!("certify_frontier_max_empties: {max_empties}");
        }
        println!("verifier_hits: {}", report.verifier_hits);
        let root_state = inspect_certify_root_state(&moves)?;
        print_root_state(&root_state);
        println!("explanation: {}", report.explanation);
        total_nodes_visited = total_nodes_visited.saturating_add(report.nodes_visited);
        println!("total_nodes_visited: {total_nodes_visited}");

        let progress_signature = (
            usize::from(report.inserted || report.updated || report.certified),
            root_state.frontier_remaining,
            count_exact_root_children(&root_state),
        );
        if Some(progress_signature) == last_progress_signature {
            stall_count += 1;
        } else {
            stall_count = 0;
            last_progress_signature = Some(progress_signature);
        }
        if follow_until_stall {
            println!("stall_count: {stall_count}/{stall_limit}");
        }

        if report.certified {
            if follow_blockers
                && let Some(ancestor_moves) = nearest_unresolved_ancestor(&moves)?
            {
                println!("backtrack_to: {}", format_moves(&ancestor_moves));
                moves = ancestor_moves;
                println!();
                continue;
            }
            break;
        }

        if let Some(limit) = total_max_nodes
            && total_nodes_visited >= limit
        {
            println!("stop_reason: total node budget exhausted");
            break;
        }

        let follow_limit_reached = follow_steps.is_some_and(|limit| followed >= limit);
        if follow_blockers && !follow_limit_reached {
            if follow_until_stall && stall_count >= stall_limit {
                println!("stop_reason: stalled");
                break;
            }
            if let Some(next_moves) = dominant_blocker_extension(&root_state) {
                println!("follow_blocker: {}", format_moves(&next_moves));
                moves.extend(next_moves);
                followed += 1;
                println!();
                continue;
            }
            println!("follow_blocker: none");
            break;
        }

        if follow_until_stall {
            if stall_count >= stall_limit {
                println!("stop_reason: stalled");
            } else {
                println!("stop_reason: follow limit reached");
            }
            break;
        }

        if auto_double {
            if max_nodes > usize::MAX / 2 {
                return Err("cannot auto-double max_nodes further without overflowing usize".to_string());
            }
            max_nodes *= 2;
            attempt += 1;
            println!();
            continue;
        }

        break;
    }

    Ok(())
}

fn dominant_blocker_extension(
    report: &crate::expander::CertifyRootStateReport,
) -> Option<Vec<Column>> {
    let child = report
        .root_children
        .iter()
        .filter(|child| child.exact_value.is_none())
        .max_by(|left, right| {
            left.frontier_count
                .cmp(&right.frontier_count)
                .then(left.attempted_unresolved_children.cmp(&right.attempted_unresolved_children))
                .then_with(|| match (left.frontier_min_empties, right.frontier_min_empties) {
                    (Some(l), Some(r)) => r.cmp(&l),
                    (Some(_), None) => std::cmp::Ordering::Greater,
                    (None, Some(_)) => std::cmp::Ordering::Less,
                    (None, None) => std::cmp::Ordering::Equal,
                })
                .then(right.column.0.cmp(&left.column.0))
        })?;

    let mut next = vec![child.column];
    if let Some(grandchild) = child
        .child_statuses
        .iter()
        .max_by(|left, right| {
            left.frontier_count
                .cmp(&right.frontier_count)
                .then_with(|| match (left.frontier_min_empties, right.frontier_min_empties) {
                    (Some(l), Some(r)) => r.cmp(&l),
                    (Some(_), None) => std::cmp::Ordering::Greater,
                    (None, Some(_)) => std::cmp::Ordering::Less,
                    (None, None) => std::cmp::Ordering::Equal,
                })
                .then(
                    matches!(left.status, crate::bookdb::CertifyChildStatus::AttemptedUnresolved)
                        .cmp(&matches!(
                            right.status,
                            crate::bookdb::CertifyChildStatus::AttemptedUnresolved
                        )),
                )
                .then(right.column.0.cmp(&left.column.0))
        })
        .filter(|grandchild| {
            grandchild.frontier_count > 0
                || matches!(
                    grandchild.status,
                    crate::bookdb::CertifyChildStatus::AttemptedUnresolved
                )
        })
    {
        next.push(grandchild.column);
    }
    Some(next)
}

fn print_root_state(report: &crate::expander::CertifyRootStateReport) {
    println!("position: {}", report.position_label);
    println!("certify_frontier_remaining: {}", report.frontier_remaining);
    println!("root_children:");
    for child in &report.root_children {
        let status = child
            .exact_value
            .map(|value| format!("{value:?}"))
            .unwrap_or_else(|| "Unresolved".to_string());
        println!(
            "  {}: {} (known_children: {}/{}, attempted_unresolved: {}, frontier: {}, frontier_min_empties: {})",
            (b'a' + child.column.0 as u8) as char,
            status,
            child.known_children,
            child.total_children,
            child.attempted_unresolved_children,
            child.frontier_count,
            child
                .frontier_min_empties
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string())
        );
        if !child.child_statuses.is_empty() {
            let statuses = child
                .child_statuses
                .iter()
                .map(|grandchild| {
                    format!(
                        "{}={:?}[frontier:{},min:{}]",
                        (b'a' + grandchild.column.0 as u8) as char,
                        grandchild.status,
                        grandchild.frontier_count,
                        grandchild
                            .frontier_min_empties
                            .map(|value| value.to_string())
                            .unwrap_or_else(|| "-".to_string())
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            println!("    child_statuses: {statuses}");
        }
    }
}

fn format_moves(moves: &[Column]) -> String {
    moves.iter()
        .map(|column| (b'a' + column.0 as u8) as char)
        .collect::<String>()
}

fn count_exact_root_children(report: &crate::expander::CertifyRootStateReport) -> usize {
    report
        .root_children
        .iter()
        .filter(|child| child.exact_value.is_some())
        .count()
}

fn nearest_unresolved_ancestor(moves: &[Column]) -> Result<Option<Vec<Column>>, String> {
    let store = RedbBookStore::open_or_create(default_book_db_path())
        .map_err(|err| format!("failed to open book db: {err}"))?;
    let mut position = Position::new();
    let mut prefixes = Vec::with_capacity(moves.len());
    for &column in moves {
        position
            .apply_move(column)
            .map_err(|err| format!("cannot apply move while finding ancestor: {err}"))?;
        prefixes.push((prefixes.len() + 1, position.clone()));
    }

    for (len, position) in prefixes.into_iter().rev().skip(1) {
        let exact = store
            .get(&position)
            .map_err(|err| format!("failed to query book db: {err}"))?
            .and_then(|entry| entry.exact_value)
            .is_some();
        if !exact {
            return Ok(Some(moves[..len].to_vec()));
        }
    }

    Ok(None)
}

const BOOK_REPORT_SNAPSHOT_PATH: &str = "book_report_snapshot.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BookSnapshot {
    total_entries: usize,
    by_ply: Vec<usize>,
}

fn cmd_report_book<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    let mut update_snapshot = false;
    for arg in args {
        match arg.as_str() {
            "--update-snapshot" => update_snapshot = true,
            _ => return Err(format!("invalid argument `{arg}`")),
        }
    }

    let store = RedbBookStore::open_or_create(default_book_db_path())
        .map_err(|err| format!("failed to open book db: {err}"))?;
    let entries = store
        .all_entries()
        .map_err(|err| format!("failed to read book db entries: {err}"))?;
    let mut by_side = BTreeMap::<&'static str, usize>::new();
    let mut by_verdict = BTreeMap::<&'static str, usize>::new();
    let mut by_source = BTreeMap::<&'static str, usize>::new();
    let mut by_ply = [0usize; 43];
    let mut by_bucket = BTreeMap::<String, usize>::new();

    for entry in &entries {
        *by_side.entry(side_name(entry.side_to_move)).or_default() += 1;
        *by_verdict.entry(verdict_name(&entry.verdict)).or_default() += 1;
        *by_source.entry(source_name(&entry.source)).or_default() += 1;

        let ply = entry.canonical_key.iter().filter(|&&cell| cell != 0).count();
        by_ply[ply] += 1;
        let bucket_start = (ply / 2) * 2;
        *by_bucket
            .entry(format!("{bucket_start:02}-{end:02}", end = bucket_start + 1))
            .or_default() += 1;
    }

    println!("entries: {}", entries.len());
    println!("side_to_move:");
    for (side, count) in by_side {
        println!("  {side}: {count}");
    }
    println!("verdict:");
    for (verdict, count) in by_verdict {
        println!("  {verdict}: {count}");
    }
    println!("source:");
    for (source, count) in by_source {
        println!("  {source}: {count}");
    }
    println!("ply_distribution:");
    for (ply, count) in by_ply.iter().enumerate().filter(|(_, count)| **count > 0) {
        println!("  {ply:>2}: {count}");
    }
    println!("ply_buckets:");
    for (bucket, count) in by_bucket {
        println!("  {bucket}: {count}");
    }

    let snapshot = BookSnapshot {
        total_entries: entries.len(),
        by_ply: by_ply.to_vec(),
    };

    if let Some(previous) = load_report_snapshot()? {
        println!("delta_since_snapshot:");
        println!(
            "  entries: {:+}",
            snapshot.total_entries as isize - previous.total_entries as isize
        );
        for (ply, current) in snapshot.by_ply.iter().enumerate() {
            let previous_count = previous.by_ply.get(ply).copied().unwrap_or_default();
            let delta = *current as isize - previous_count as isize;
            if delta != 0 {
                println!("  ply {ply:>2}: {delta:+}");
            }
        }
    } else {
        println!("delta_since_snapshot:");
        println!("  no snapshot available");
    }

    if update_snapshot {
        save_report_snapshot(&snapshot)?;
        println!("snapshot_updated: {}", default_report_snapshot_path().display());
    }

    Ok(())
}

fn cmd_migrate_book_db() -> Result<(), String> {
    let book = Book::new();
    let store = RedbBookStore::open_or_create(default_book_db_path())
        .map_err(|err| format!("failed to open redb store: {err}"))?;
    let imported = store
        .import_book(&book)
        .map_err(|err| format!("failed to import book into redb: {err}"))?;
    let total = store
        .len()
        .map_err(|err| format!("failed to count redb entries: {err}"))?;

    println!("imported_entries: {imported}");
    println!("redb_entries: {total}");
    println!("redb_path: {}", default_book_db_path().display());
    Ok(())
}

fn position_from_moves<I>(args: I) -> Result<Position, String>
where
    I: IntoIterator<Item = String>,
{
    let mut position = Position::new();
    for arg in args {
        let column = parse_column(&arg)?;
        position
            .apply_move(column)
            .map_err(|err| format!("cannot apply move `{arg}`: {err}"))?;
    }
    Ok(position)
}

fn parse_column(raw: &str) -> Result<Column, String> {
    let trimmed = raw.trim();
    if trimmed.len() == 1 {
        let ch = trimmed.chars().next().unwrap();
        if ('a'..='g').contains(&ch) {
            return Ok(Column((ch as u8 - b'a') as usize));
        }
    }

    trimmed
        .parse::<usize>()
        .map_err(|_| format!("invalid column `{raw}`"))
        .and_then(|value| {
            if (1..=7).contains(&value) {
                Ok(Column(value - 1))
            } else {
                Err(format!("column out of range `{raw}`"))
            }
        })
}

fn print_usage() {
    println!("usage:");
    println!("  connect4_knowledge gui");
    println!("  connect4_knowledge expand-book [batch_limit] [--reset-frontier] [--checkpoint-every N]");
    println!("  connect4_knowledge expand-branch [moves...] [--batch-limit N] [--reset-frontier] [--checkpoint-every N]");
    println!("  connect4_knowledge certify-position [moves...] [--max-nodes N] [--auto-double] [--dump-root-state] [--follow-blockers] [--follow-steps N] [--follow-until-stall] [--stall-limit N] [--total-max-nodes N]");
    println!("  connect4_knowledge play [moves...]");
    println!("  connect4_knowledge analyze [moves...]");
    println!("  connect4_knowledge explain [moves...]");
    println!("  connect4_knowledge dump-proof [moves...]");
    println!("  connect4_knowledge report-book [--update-snapshot]");
    println!("  connect4_knowledge migrate-book-db");
}

fn side_name(side: crate::board::Color) -> &'static str {
    match side {
        crate::board::Color::White => "White",
        crate::board::Color::Black => "Black",
    }
}

fn verdict_name(verdict: &Verdict) -> &'static str {
    match verdict {
        Verdict::SolvedWin => "SolvedWin",
        Verdict::SolvedDraw => "SolvedDraw",
        Verdict::Unresolved => "Unresolved",
    }
}

fn source_name(source: &EntrySource) -> &'static str {
    match source {
        EntrySource::Book => "Book",
        EntrySource::RuleProof => "RuleProof",
        EntrySource::Verifier => "Verifier",
    }
}

fn default_report_snapshot_path() -> PathBuf {
    PathBuf::from(BOOK_REPORT_SNAPSHOT_PATH)
}

fn load_report_snapshot() -> Result<Option<BookSnapshot>, String> {
    let path = default_report_snapshot_path();
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(&path).map_err(|err| format!("failed to read snapshot: {err}"))?;
    let snapshot = serde_json::from_str(&contents).map_err(|err| format!("failed to parse snapshot: {err}"))?;
    Ok(Some(snapshot))
}

fn save_report_snapshot(snapshot: &BookSnapshot) -> Result<(), String> {
    let path = default_report_snapshot_path();
    let contents =
        serde_json::to_string_pretty(snapshot).map_err(|err| format!("failed to encode snapshot: {err}"))?;
    fs::write(path, contents).map_err(|err| format!("failed to write snapshot: {err}"))
}
