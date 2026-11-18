//! Custom dictionary storage and STT-prompt priority.
//!
//! Every edit goes through one delta path ([`Database::apply_dictionary_changes`]),
//! which touches only the words it names. Words are unique case-insensitively.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use unicode_normalization::UnicodeNormalization;

use super::Database;
use super::timestamp::{now_db_timestamp, parse_db_timestamp};
use crate::analytics::trim_text;
use crate::text::correction_learner::{Correction, KnownCorrection};
use crate::text::script::is_unspaced_script;
use crate::text::term_match::continues_word;

/// Who added a dictionary word. Manual words outrank learned ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DictionarySource {
    #[default]
    Manual,
    /// Added by auto-learn from the user's corrections.
    Learned,
}

impl DictionarySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Learned => "learned",
        }
    }

    fn from_db(value: &str) -> Self {
        if value == Self::Learned.as_str() {
            Self::Learned
        } else {
            Self::Manual
        }
    }
}

/// A dictionary word with the metadata the Dictionary view shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryEntry {
    pub word: String,
    pub source: DictionarySource,
    pub use_count: i64,
    pub last_used_at: Option<DateTime<Utc>>,
}

/// Newly remembered vocabulary and corrections that need the current context.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CorrectionLearningOutcome {
    pub added_terms: Vec<String>,
    pub context_dependent_terms: Vec<String>,
    pub context_dependent_pairs: Vec<Correction>,
    pub clear_correction_pairs: Vec<Correction>,
}

impl DictionaryEntry {
    /// Projects a queued edit without waiting for SQLite. Normalization and
    /// insertion order match `apply_dictionary_changes`; stored metadata is
    /// reconciled after the write finishes.
    pub fn project_changes<A: AsRef<str>, R: AsRef<str>>(
        entries: &mut Vec<Self>,
        add: &[A],
        remove: &[R],
        source: DictionarySource,
    ) {
        let additions = normalize_words(add);
        let addition_keys: HashSet<&str> = additions.iter().map(|(key, _)| key.as_str()).collect();
        let removals: HashSet<String> = normalize_words(remove)
            .into_iter()
            .map(|(key, _)| key)
            .filter(|key| !addition_keys.contains(key.as_str()))
            .collect();
        let before: HashMap<String, usize> = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.word.to_lowercase(), index))
            .collect();
        let removed: HashSet<usize> = removals
            .iter()
            .filter_map(|key| before.get(key).copied())
            .collect();
        let mut index = 0;
        entries.retain(|_| {
            let keep = !removed.contains(&index);
            index += 1;
            keep
        });
        let positions: HashMap<String, usize> = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.word.to_lowercase(), index))
            .collect();
        for (key, word) in additions {
            if let Some(&index) = positions.get(&key) {
                let entry = &mut entries[index];
                entry.word = word;
                if source == DictionarySource::Manual {
                    entry.source = source;
                }
            } else {
                entries.push(Self {
                    word,
                    source,
                    use_count: 0,
                    last_used_at: None,
                });
            }
        }
    }
}

const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// Freshness tiers by last use: within a day, within two weeks, older.
const FRESHNESS_TIER_LIMITS_MS: [i64; 2] = [DAY_MS, 14 * DAY_MS];

/// Counts an already-normalized term without lowercasing every candidate
/// character again. Rejected boundary matches advance one character so a
/// later overlapping match can still be accepted.
fn count_normalized_term(text: &str, term: &str) -> usize {
    let whole_word = !term.chars().any(is_unspaced_script);
    let (mut cursor, mut count) = (0, 0);
    while let Some(offset) = text[cursor..].find(term) {
        let start = cursor + offset;
        let end = start + term.len();
        let boundary_match = !whole_word
            || (!text[..start]
                .chars()
                .next_back()
                .is_some_and(continues_word)
                && !text[end..].chars().next().is_some_and(continues_word));
        if boundary_match {
            count += 1;
            cursor = end;
        } else {
            cursor = start + text[start..].chars().next().unwrap().len_utf8();
        }
    }
    count
}

/// A dictionary row as the priority ranking sees it.
#[derive(Debug, Clone, Default)]
struct DictionaryRankRow {
    id: i64,
    word: String,
    source: String,
    use_count: i64,
    last_used_at: Option<String>,
    updated_at: String,
}

fn freshness_tier(row: &DictionaryRankRow, now_ms: i64) -> usize {
    let last_touched = row.last_used_at.as_deref().unwrap_or(&row.updated_at);
    match parse_db_timestamp(last_touched) {
        Some(t) => {
            let age = now_ms - t.timestamp_millis();
            FRESHNESS_TIER_LIMITS_MS
                .iter()
                .position(|limit| age <= *limit)
                .unwrap_or(FRESHNESS_TIER_LIMITS_MS.len())
        }
        None => FRESHNESS_TIER_LIMITS_MS.len(),
    }
}

/// Words highest priority first: manual before learned, then by freshness of
/// last use (falling back to the last edit), then use count, then newest.
fn rank_dictionary_rows(mut rows: Vec<DictionaryRankRow>, now_ms: i64) -> Vec<String> {
    rows.sort_by_cached_key(|row| {
        (
            row.source == DictionarySource::Learned.as_str(),
            freshness_tier(row, now_ms),
            std::cmp::Reverse(row.use_count),
            std::cmp::Reverse(row.id),
        )
    });
    rows.into_iter().map(|row| row.word).collect()
}

/// Trimmed, deduplicated by lowercase (first spelling wins), in input order.
fn normalize_words<S: AsRef<str>>(words: &[S]) -> Vec<(String, String)> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for raw in words {
        let trimmed = trim_text(raw.as_ref());
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_lowercase();
        if seen.insert(lower.clone()) {
            out.push((lower, trimmed.to_string()));
        }
    }
    out
}

#[derive(Debug)]
struct ExistingRow {
    id: i64,
    word: String,
    source: String,
}

struct DictionaryUsage {
    id: i64,
    word: String,
    updated_at: String,
    count: usize,
}

/// All rows, indexed by lowercase word.
fn dictionary_rows(conn: &Connection) -> Result<HashMap<String, ExistingRow>> {
    let rows = conn
        .prepare("SELECT id, word, source FROM custom_dictionary")?
        .query_map([], |row| {
            let word: String = row.get(1)?;
            Ok((
                word.to_lowercase(),
                ExistingRow {
                    id: row.get(0)?,
                    word,
                    source: row.get(2)?,
                },
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Inserts a word or updates its spelling and manual source when needed.
fn upsert_word(
    tx: &Transaction<'_>,
    word: &str,
    existing: Option<&ExistingRow>,
    source: &str,
) -> Result<()> {
    let Some(existing) = existing else {
        tx.prepare_cached("INSERT INTO custom_dictionary (word, source) VALUES (?, ?)")?
            .execute(params![word, source])?;
        return Ok(());
    };
    let now = now_db_timestamp();
    if source == "manual" && existing.source == "learned" {
        tx.prepare_cached(
            "UPDATE custom_dictionary SET word = ?, source = 'manual', updated_at = ? WHERE id = ?",
        )?
        .execute(params![word, now, existing.id])?;
    } else if existing.word != word {
        // A respelling; an unchanged row keeps its updated_at.
        tx.prepare_cached("UPDATE custom_dictionary SET word = ?, updated_at = ? WHERE id = ?")?
            .execute(params![word, now, existing.id])?;
    }
    if existing.word != word {
        // SQLite NOCASE cannot follow a Unicode case-only respelling.
        tx.prepare_cached("UPDATE dictionary_corrections SET term = ? WHERE term = ?")?
            .execute(params![word, existing.word])?;
        tx.prepare_cached("UPDATE dictionary_session_corrections SET term = ? WHERE term = ?")?
            .execute(params![word, existing.word])?;
    }
    Ok(())
}

fn rejected_words(conn: &Connection) -> Result<Vec<String>> {
    Ok(conn
        .prepare("SELECT word FROM dictionary_rejected_words")?
        .query_map([], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

fn respell_case_correction(
    tx: &Transaction<'_>,
    existing: &mut ExistingRow,
    misheard: &str,
    term: &str,
) -> Result<()> {
    if misheard.to_lowercase() == term.to_lowercase() {
        upsert_word(tx, term, Some(existing), DictionarySource::Learned.as_str())?;
        existing.word = term.to_string();
    }
    Ok(())
}

fn delete_row(tx: &Transaction<'_>, existing: Option<&ExistingRow>) -> Result<()> {
    let Some(existing) = existing else {
        return Ok(());
    };
    let deleted = tx
        .prepare_cached("DELETE FROM custom_dictionary WHERE id = ?")?
        .execute([existing.id])?;
    if deleted == 0 {
        return Ok(());
    }
    // What the user took out stays out, and its mishearings stop applying.
    tx.prepare_cached("DELETE FROM dictionary_corrections WHERE term = ?")?
        .execute([&existing.word])?;
    tx.prepare_cached("DELETE FROM dictionary_session_corrections WHERE term = ?")?
        .execute([&existing.word])?;
    tx.prepare_cached("DELETE FROM dictionary_session_terms WHERE word_id = ?")?
        .execute([existing.id])?;
    tx.prepare_cached("INSERT OR IGNORE INTO dictionary_rejected_words (word) VALUES (?)")?
        .execute([&existing.word])?;
    Ok(())
}

impl Database {
    /// Words in insertion order.
    #[cfg(test)]
    pub fn get_dictionary(&self) -> Result<Vec<String>> {
        let conn = self.lock();
        let words = conn
            .prepare("SELECT word FROM custom_dictionary ORDER BY id ASC")?
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(words)
    }

    /// Words with their source and usage, in insertion order.
    pub fn get_dictionary_entries(&self) -> Result<Vec<DictionaryEntry>> {
        let conn = self.lock();
        let entries = conn
            .prepare(
                "SELECT word, source, use_count, last_used_at FROM custom_dictionary ORDER BY id ASC",
            )?
            .query_map([], |row| {
                let source: String = row.get(1)?;
                let last_used_at: Option<String> = row.get(3)?;
                Ok(DictionaryEntry {
                    word: row.get(0)?,
                    source: DictionarySource::from_db(&source),
                    use_count: row.get(2)?,
                    last_used_at: last_used_at.as_deref().and_then(parse_db_timestamp),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(entries)
    }

    /// Words highest priority first, for picking what fits in the STT prompt.
    pub fn get_dictionary_by_priority(&self) -> Result<Vec<String>> {
        let conn = self.lock();
        let rows = conn
            .prepare(
                "SELECT id, word, source, use_count, last_used_at, updated_at FROM custom_dictionary",
            )?
            .query_map([], |row| {
                Ok(DictionaryRankRow {
                    id: row.get(0)?,
                    word: row.get(1)?,
                    source: row.get(2)?,
                    use_count: row.get(3)?,
                    last_used_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(conn);
        Ok(rank_dictionary_rows(rows, Utc::now().timestamp_millis()))
    }

    /// Counts the dictionary words a delivered dictation used. Usage is not an
    /// edit, so `updated_at` stays as it was. Returns how many distinct words
    /// were used.
    pub fn record_dictionary_usage(&self, text: &str) -> Result<usize> {
        if text.is_empty() {
            return Ok(0);
        }
        let mut usage: Vec<DictionaryUsage> = {
            let conn = self.lock();
            conn.prepare("SELECT id, word, updated_at FROM custom_dictionary")?
                .query_map([], |row| {
                    Ok(DictionaryUsage {
                        id: row.get(0)?,
                        word: row.get(1)?,
                        updated_at: row.get(2)?,
                        count: 0,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?
        };
        // Matching can scan a long transcript once per dictionary word. Keep
        // it outside the connection mutex so reads and edits can proceed.
        let transcript = text.nfc().collect::<String>().to_lowercase();
        usage.retain_mut(|row| {
            let word = trim_text(&row.word)
                .nfc()
                .collect::<String>()
                .to_lowercase();
            if word.is_empty() {
                return false;
            }
            row.count = count_normalized_term(&transcript, &word);
            row.count > 0
        });
        self.apply_dictionary_usage(&usage)
    }

    fn apply_dictionary_usage(&self, usage: &[DictionaryUsage]) -> Result<usize> {
        if usage.is_empty() {
            return Ok(0);
        }
        let mut conn = self.lock();
        let now = now_db_timestamp();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut updated = 0;
        {
            let mut record = tx.prepare(
                "UPDATE custom_dictionary SET use_count = use_count + ?, last_used_at = ?
                 WHERE id = ? AND word = ? COLLATE BINARY AND updated_at = ?",
            )?;
            for row in usage {
                // A removed/replaced or respelled row must not inherit a match
                // computed from its previous spelling.
                updated += record.execute(params![
                    row.count as i64,
                    now,
                    row.id,
                    row.word,
                    row.updated_at
                ])?;
            }
        }
        tx.commit()?;
        Ok(updated)
    }

    /// Adds and/or removes specific words, leaving every other row untouched.
    /// `source` tags additions. A word on both sides is kept.
    pub fn apply_dictionary_changes<A: AsRef<str>, R: AsRef<str>>(
        &self,
        add: &[A],
        remove: &[R],
        source: DictionarySource,
    ) -> Result<()> {
        let additions = normalize_words(add);
        let addition_keys: HashSet<&str> = additions.iter().map(|(l, _)| l.as_str()).collect();
        let removals: Vec<(String, String)> = normalize_words(remove)
            .into_iter()
            .filter(|(lower, _)| !addition_keys.contains(lower.as_str()))
            .collect();
        if additions.is_empty() && removals.is_empty() {
            return Ok(());
        }

        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let by_lower = dictionary_rows(&tx)?;
        if source == DictionarySource::Manual {
            for rejected in rejected_words(&tx)? {
                if addition_keys.contains(rejected.to_lowercase().as_str()) {
                    tx.prepare_cached("DELETE FROM dictionary_rejected_words WHERE word = ?")?
                        .execute([rejected])?;
                }
            }
        }
        for (lower, _) in &removals {
            delete_row(&tx, by_lower.get(lower))?;
        }
        for (lower, word) in &additions {
            upsert_word(&tx, word, by_lower.get(lower), source.as_str())?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Records the first edit of a fresh session for test fixtures.
    #[cfg(test)]
    pub fn record_corrections(&self, corrections: &[Correction]) -> Result<Vec<String>> {
        Ok(self
            .record_session_corrections_if_current(
                &uuid::Uuid::new_v4().to_string(),
                1,
                corrections,
                &AtomicU64::new(0),
                0,
            )?
            .added_terms)
    }

    /// Reconciles the latest completed edit of one observation session.
    /// Repeated snapshots and stale revisions cannot add evidence or feedback.
    pub fn record_session_corrections_if_current(
        &self,
        session_id: &str,
        revision: u64,
        corrections: &[Correction],
        generation: &AtomicU64,
        expected: u64,
    ) -> Result<CorrectionLearningOutcome> {
        let mut conn = self.lock();
        if generation.load(Ordering::Acquire) != expected {
            return Ok(CorrectionLearningOutcome::default());
        }
        let revision = i64::try_from(revision)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous_revision: Option<i64> = tx
            .query_row(
                "SELECT revision FROM dictionary_learning_sessions WHERE session_id = ?",
                [session_id],
                |row| row.get(0),
            )
            .optional()?;
        if previous_revision.is_some_and(|previous| revision <= previous) {
            return Ok(CorrectionLearningOutcome::default());
        }
        let previous: Vec<(String, String)> = tx
            .prepare(
                "SELECT misheard, term FROM dictionary_session_corrections WHERE session_id = ?",
            )?
            .query_map([session_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let previous_keys: HashSet<_> = previous
            .iter()
            .map(|(misheard, term)| (misheard.to_lowercase(), term.to_lowercase()))
            .collect();
        let rejected: HashSet<String> = rejected_words(&tx)?
            .into_iter()
            .map(|word| word.to_lowercase())
            .collect();
        let mut by_lower = dictionary_rows(&tx)?;
        let mut outcome = CorrectionLearningOutcome::default();
        let mut seen = HashSet::new();
        for correction in corrections {
            let term = trim_text(&correction.term);
            let lower = term.to_lowercase();
            let key = (correction.misheard.to_lowercase(), lower.clone());
            if term.is_empty() || rejected.contains(&lower) {
                continue;
            }
            if !seen.insert(key.clone()) || previous_keys.contains(&key) {
                if let Some(existing) = by_lower.get_mut(&lower) {
                    respell_case_correction(&tx, existing, &correction.misheard, term)?;
                }
                continue;
            }
            let known = tx
                .query_row(
                    "SELECT 1 FROM dictionary_corrections WHERE misheard = ? AND term = ?",
                    params![correction.misheard, term],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            tx.execute(
                "INSERT INTO dictionary_corrections (misheard, term) VALUES (?, ?)
                 ON CONFLICT (misheard, term) DO UPDATE SET count = count + 1, updated_at = excluded.updated_at",
                params![correction.misheard, term],
            )?;
            tx.execute(
                "INSERT INTO dictionary_session_corrections (session_id, misheard, term) VALUES (?, ?, ?)",
                params![session_id, correction.misheard, term],
            )?;
            let automatic = (KnownCorrection {
                misheard: correction.misheard.clone(),
                term: term.to_string(),
                count: 1,
            })
            .applies_automatically();
            if !known && automatic {
                outcome.clear_correction_pairs.push(correction.clone());
            }
            if !known && !automatic {
                if !outcome
                    .context_dependent_terms
                    .iter()
                    .any(|word| word.to_lowercase() == lower)
                {
                    outcome.context_dependent_terms.push(term.to_string());
                }
                outcome.context_dependent_pairs.push(correction.clone());
            }
            if let Some(existing) = by_lower.get_mut(&lower) {
                respell_case_correction(&tx, existing, &correction.misheard, term)?;
            } else {
                upsert_word(&tx, term, None, DictionarySource::Learned.as_str())?;
                tx.execute(
                    "INSERT INTO dictionary_session_terms (word_id) VALUES (?)",
                    [tx.last_insert_rowid()],
                )?;
                outcome.added_terms.push(term.to_string());
                by_lower = dictionary_rows(&tx)?;
            }
        }
        for (misheard, term) in previous {
            if seen.contains(&(misheard.to_lowercase(), term.to_lowercase())) {
                continue;
            }
            tx.execute(
                "UPDATE dictionary_corrections SET count = count - 1 WHERE misheard = ? AND term = ?",
                params![misheard, term],
            )?;
            tx.execute(
                "DELETE FROM dictionary_corrections WHERE misheard = ? AND term = ? AND count <= 0",
                params![misheard, term],
            )?;
            tx.execute(
                "DELETE FROM dictionary_session_corrections WHERE session_id = ? AND misheard = ? AND term = ?",
                params![session_id, misheard, term],
            )?;
        }
        tx.execute(
            "DELETE FROM custom_dictionary WHERE source = 'learned'
             AND id IN (SELECT word_id FROM dictionary_session_terms)
             AND NOT EXISTS (SELECT 1 FROM dictionary_corrections c WHERE c.term = custom_dictionary.word)",
            [],
        )?;
        tx.execute(
            "DELETE FROM dictionary_session_terms WHERE word_id NOT IN (SELECT id FROM custom_dictionary)",
            [],
        )?;
        tx.execute(
            "INSERT INTO dictionary_learning_sessions (session_id, revision) VALUES (?, ?)
             ON CONFLICT (session_id) DO UPDATE SET revision = excluded.revision",
            params![session_id, revision],
        )?;
        tx.commit()?;
        Ok(outcome)
    }

    /// Corrections to terms in the Dictionary, most repeated first. A mishearing
    /// that is itself a Dictionary word is not a mishearing, so it is left out.
    pub fn get_known_corrections(&self) -> Result<Vec<KnownCorrection>> {
        let conn = self.lock();
        let corrections = conn
            .prepare(
                "SELECT c.misheard, d.word, c.count
                 FROM dictionary_corrections c
                 JOIN custom_dictionary d ON d.word = c.term
                 WHERE NOT EXISTS (
                   SELECT 1 FROM custom_dictionary other
                   WHERE other.word = c.misheard AND other.id != d.id
                 )
                 ORDER BY c.count DESC, c.updated_at DESC, c.id DESC",
            )?
            .query_map([], |row| {
                Ok(KnownCorrection {
                    misheard: row.get(0)?,
                    term: row.get(1)?,
                    count: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(corrections)
    }

    /// Empties the Dictionary and everything learned about it. A fresh start,
    /// so unlike removing words it leaves auto-learn free to add them again.
    pub fn clear_dictionary(&self) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM custom_dictionary", [])?;
        tx.execute("DELETE FROM dictionary_corrections", [])?;
        tx.execute("DELETE FROM dictionary_rejected_words", [])?;
        tx.execute("DELETE FROM dictionary_session_corrections", [])?;
        tx.execute("DELETE FROM dictionary_session_terms", [])?;
        tx.execute("DELETE FROM dictionary_learning_sessions", [])?;
        tx.commit()?;
        Ok(())
    }

    /// Forgets one mishearing of `term`.
    pub fn remove_correction(&self, misheard: &str, term: &str) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM dictionary_corrections WHERE misheard = ? AND term = ?",
            params![misheard, term],
        )?;
        tx.execute(
            "DELETE FROM dictionary_session_corrections WHERE misheard = ? AND term = ?",
            params![misheard, term],
        )?;
        // Explicitly keeping the word while forgetting its correction makes it
        // independent of any older observation session.
        tx.execute(
            "DELETE FROM dictionary_session_terms WHERE word_id IN (SELECT id FROM custom_dictionary WHERE word = ?)",
            [term],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The stored source of a word, if present.
    #[cfg(test)]
    pub fn dictionary_word_source(&self, word: &str) -> Result<Option<String>> {
        use rusqlite::OptionalExtension;
        let conn = self.lock();
        Ok(conn
            .query_row(
                "SELECT source FROM custom_dictionary WHERE word = ?",
                [word],
                |row| row.get(0),
            )
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::timestamp::format_db_timestamp;
    use chrono::DateTime;

    const NONE: &[&str] = &[];

    fn db() -> Database {
        Database::open_in_memory().unwrap()
    }

    fn manual() -> DictionarySource {
        DictionarySource::Manual
    }

    #[test]
    fn failed_clear_rolls_back_and_releases_the_shared_connection() {
        let db = db();
        db.apply_dictionary_changes(&["keep"], NONE, manual())
            .unwrap();
        db.lock()
            .execute_batch(
                "CREATE TRIGGER reject_clear BEFORE DELETE ON dictionary_corrections
             BEGIN SELECT RAISE(ABORT, 'synthetic clear failure'); END;
             INSERT INTO dictionary_corrections(misheard, term, count) VALUES ('keap', 'keep', 1);",
            )
            .unwrap();
        assert!(db.clear_dictionary().is_err());
        assert!(db.lock().is_autocommit());
        assert_eq!(db.get_dictionary().unwrap(), ["keep"]);
        db.lock()
            .execute_batch("DROP TRIGGER reject_clear;")
            .unwrap();
        db.apply_dictionary_changes(&["next"], NONE, manual())
            .unwrap();
        db.clear_dictionary().unwrap();
        assert!(db.get_dictionary().unwrap().is_empty());
    }

    #[test]
    fn immediate_edits_match_sqlite_across_rapid_case_and_source_changes() {
        let db = Database::open_in_memory().unwrap();
        let mut projected = Vec::new();
        let edits = [
            (
                vec![" Alpha ", "BETA", "Éclair", "İstanbul"],
                vec![],
                DictionarySource::Learned,
            ),
            (
                vec!["alpha", "Gamma", "éclair", "i̇stanbul"],
                vec!["ALPHA", "Beta"],
                DictionarySource::Manual,
            ),
            (
                vec!["GAMMA", "\u{feff}新词\u{feff}", "", "新词"],
                vec!["missing", "alpha"],
                DictionarySource::Learned,
            ),
            (
                vec!["Alpha", "BETA"],
                vec!["éclair", "GAMMA"],
                DictionarySource::Manual,
            ),
            (
                vec!["beta"],
                vec!["BETA", "İSTANBUL"],
                DictionarySource::Learned,
            ),
        ];
        for (add, remove, source) in edits {
            DictionaryEntry::project_changes(&mut projected, &add, &remove, source);
            db.apply_dictionary_changes(&add, &remove, source).unwrap();
            assert_eq!(projected, db.get_dictionary_entries().unwrap());
        }
        db.record_dictionary_usage("Alpha beta").unwrap();
        projected = db.get_dictionary_entries().unwrap();
        DictionaryEntry::project_changes(
            &mut projected,
            &["ALPHA"],
            &[] as &[&str],
            DictionarySource::Learned,
        );
        db.apply_dictionary_changes(&["ALPHA"], &[] as &[&str], DictionarySource::Learned)
            .unwrap();
        assert_eq!(
            projected,
            db.get_dictionary_entries().unwrap(),
            "respelling preserves usage and source"
        );
    }

    #[test]
    fn projection_preserves_sqlite_non_ascii_case_distinctions_in_legacy_rows() {
        let db = Database::open_in_memory().unwrap();
        db.lock()
            .execute_batch("INSERT INTO custom_dictionary(word) VALUES ('Éclair'), ('éclair');")
            .unwrap();
        let mut projected = db.get_dictionary_entries().unwrap();
        DictionaryEntry::project_changes(
            &mut projected,
            &[] as &[&str],
            &["ÉCLAIR"],
            DictionarySource::Manual,
        );
        db.apply_dictionary_changes(&[] as &[&str], &["ÉCLAIR"], DictionarySource::Manual)
            .unwrap();
        assert_eq!(projected, db.get_dictionary_entries().unwrap());
    }

    #[test]
    fn entries_carry_source_and_usage() {
        let db = db();
        db.apply_dictionary_changes(&["Whisper", "Alice"], NONE, manual())
            .unwrap();
        db.apply_dictionary_changes(&["Kubernetes"], NONE, DictionarySource::Learned)
            .unwrap();
        db.record_dictionary_usage("whisper and Whisper").unwrap();
        let entries = db.get_dictionary_entries().unwrap();
        let words: Vec<&str> = entries.iter().map(|e| e.word.as_str()).collect();
        assert_eq!(words, ["Whisper", "Alice", "Kubernetes"]);
        assert_eq!(entries[0].use_count, 2);
        assert!(entries[0].last_used_at.is_some());
        assert_eq!(entries[1].use_count, 0);
        assert_eq!(entries[1].last_used_at, None);
        assert_eq!(entries[1].source, DictionarySource::Manual);
        assert_eq!(entries[2].source, DictionarySource::Learned);
    }

    #[test]
    fn a_delta_touches_only_the_words_it_names() {
        let db = db();
        db.apply_dictionary_changes(&["Whisper", "Alice", "Bob"], NONE, manual())
            .unwrap();
        db.apply_dictionary_changes(&["Kubernetes", "Postgres"], NONE, DictionarySource::Learned)
            .unwrap();
        db.apply_dictionary_changes(&["Carol"], NONE, manual())
            .unwrap();
        // A stale one-word view of the list cannot delete the rest.
        db.apply_dictionary_changes(&["Whisper"], NONE, manual())
            .unwrap();
        db.apply_dictionary_changes(NONE, &["Kubernetes"], manual())
            .unwrap();
        assert_eq!(
            db.get_dictionary().unwrap(),
            ["Whisper", "Alice", "Bob", "Postgres", "Carol"]
        );
        assert_eq!(
            db.dictionary_word_source("Postgres").unwrap().as_deref(),
            Some("learned")
        );
    }

    #[test]
    fn a_manual_add_promotes_a_learned_word() {
        let db = db();
        db.apply_dictionary_changes(&["Kubernetes"], NONE, DictionarySource::Learned)
            .unwrap();
        assert_eq!(
            db.dictionary_word_source("Kubernetes").unwrap().as_deref(),
            Some("learned")
        );
        db.apply_dictionary_changes(&["Kubernetes"], NONE, manual())
            .unwrap();
        assert_eq!(
            db.dictionary_word_source("Kubernetes").unwrap().as_deref(),
            Some("manual")
        );
    }

    #[test]
    fn an_edit_is_a_remove_plus_an_add() {
        let db = db();
        db.apply_dictionary_changes(&["Whisper", "Kubernets", "Alice"], NONE, manual())
            .unwrap();
        db.apply_dictionary_changes(&["Kubernetes"], &["Kubernets"], manual())
            .unwrap();
        assert_eq!(
            db.get_dictionary().unwrap(),
            ["Whisper", "Alice", "Kubernetes"]
        );
    }

    #[test]
    fn a_word_on_both_sides_of_a_delta_is_kept() {
        let db = db();
        db.apply_dictionary_changes(&["Whisper", "Alice"], NONE, manual())
            .unwrap();
        db.apply_dictionary_changes(&["Alice", "Carol"], &["Nobody", "Alice"], manual())
            .unwrap();
        assert_eq!(db.get_dictionary().unwrap(), ["Whisper", "Alice", "Carol"]);
    }

    #[test]
    fn an_empty_delta_touches_nothing() {
        let db = db();
        db.apply_dictionary_changes(&["Whisper", "Alice"], NONE, manual())
            .unwrap();
        let snapshot = |db: &Database| -> Vec<(i64, String, Option<String>)> {
            db.lock()
                .prepare("SELECT id, word, updated_at FROM custom_dictionary ORDER BY id")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        let before = snapshot(&db);
        db.apply_dictionary_changes(NONE, NONE, manual()).unwrap();
        assert_eq!(snapshot(&db), before);
    }

    #[test]
    fn case_variant_adds_do_not_duplicate_rows() {
        let db = db();
        db.apply_dictionary_changes(&["Whisper", "Alice"], NONE, manual())
            .unwrap();
        db.apply_dictionary_changes(&["alice", "ALICE"], NONE, manual())
            .unwrap();
        let count: i64 = db
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM custom_dictionary WHERE lower(word) = 'alice'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn a_case_only_respelling_updates_the_word() {
        let db = db();
        db.apply_dictionary_changes(&["github"], NONE, manual())
            .unwrap();
        db.apply_dictionary_changes(&["GitHub"], NONE, manual())
            .unwrap();
        assert_eq!(db.get_dictionary().unwrap(), ["GitHub"]);
    }

    #[test]
    fn a_removed_word_can_be_added_again() {
        let db = db();
        db.apply_dictionary_changes(&["Alice"], NONE, manual())
            .unwrap();
        db.apply_dictionary_changes(NONE, &["alice"], manual())
            .unwrap();
        assert!(db.get_dictionary().unwrap().is_empty());
        assert_eq!(count_rows(&db), 0, "removal deletes the row");
        db.apply_dictionary_changes(&["ALICE"], NONE, manual())
            .unwrap();
        assert_eq!(db.get_dictionary().unwrap(), ["ALICE"]);
    }

    fn count_rows(db: &Database) -> i64 {
        db.lock()
            .query_row("SELECT COUNT(*) FROM custom_dictionary", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn recording_usage_is_not_an_edit() {
        let db = db();
        db.apply_dictionary_changes(&["Whisper", "Alice", "Bob"], NONE, manual())
            .unwrap();
        let updated_at = |db: &Database| -> Vec<Option<String>> {
            db.lock()
                .prepare("SELECT updated_at FROM custom_dictionary ORDER BY id")
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        let before = updated_at(&db);

        assert_eq!(
            db.record_dictionary_usage("Alice met alice and Bob")
                .unwrap(),
            2
        );

        let rows: Vec<(String, i64, bool)> = db
            .lock()
            .prepare(
                "SELECT word, use_count, last_used_at IS NOT NULL FROM custom_dictionary ORDER BY id",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            rows,
            [
                ("Whisper".into(), 0, false),
                ("Alice".into(), 2, true),
                ("Bob".into(), 1, true),
            ]
        );
        assert_eq!(updated_at(&db), before);
        assert_eq!(db.get_dictionary().unwrap(), ["Whisper", "Alice", "Bob"]);
    }

    #[test]
    fn stale_usage_does_not_credit_respelled_or_replaced_words() {
        let db = db();
        db.apply_dictionary_changes(&["github", "Alice", "Bob"], NONE, manual())
            .unwrap();
        let usage: Vec<DictionaryUsage> = db
            .lock()
            .prepare("SELECT id, word, updated_at FROM custom_dictionary ORDER BY id")
            .unwrap()
            .query_map([], |row| {
                Ok(DictionaryUsage {
                    id: row.get(0)?,
                    word: row.get(1)?,
                    updated_at: row.get(2)?,
                    count: 2,
                })
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        // Case-only changes retain the row ID; exact spelling still guards it.
        db.apply_dictionary_changes(&["GitHub"], NONE, manual())
            .unwrap();
        db.apply_dictionary_changes(NONE, &["Alice"], manual())
            .unwrap();
        db.apply_dictionary_changes(&["Alice"], NONE, manual())
            .unwrap();
        assert_eq!(db.apply_dictionary_usage(&usage).unwrap(), 1);
        let entries = db.get_dictionary_entries().unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.word.as_str(), entry.use_count))
                .collect::<Vec<_>>(),
            [("GitHub", 0), ("Bob", 2), ("Alice", 0)]
        );
    }

    #[test]
    fn usage_matching_preserves_unicode_boundaries_and_rejected_overlaps() {
        let db = db();
        db.apply_dictionary_changes(
            &["a-a", "Café", "张珊", "Claude Code", "rust", "i", "missing"],
            NONE,
            manual(),
        )
        .unwrap();
        let updated = db
            .record_dictionary_usage(
                "xa-a-a CAFE\u{301} 张珊张珊 用Claude Code写 trusted RUST rust_x İ",
            )
            .unwrap();
        assert_eq!(updated, 6);
        let entries = db.get_dictionary_entries().unwrap();
        let counts: Vec<_> = entries
            .iter()
            .map(|entry| (entry.word.as_str(), entry.use_count))
            .collect();
        assert_eq!(
            counts,
            [
                ("a-a", 1),
                ("Café", 1),
                ("张珊", 2),
                ("Claude Code", 1),
                ("rust", 1),
                ("i", 1),
                ("missing", 0)
            ]
        );
    }

    #[test]
    fn priority_favors_manual_recent_words_and_skips_deleted() {
        let db = db();
        db.apply_dictionary_changes(&["Stale", "Busy", "Gone"], NONE, manual())
            .unwrap();
        db.apply_dictionary_changes(&["Learned"], NONE, DictionarySource::Learned)
            .unwrap();
        let stale = format_db_timestamp(&(Utc::now() - chrono::Duration::days(60)));
        db.lock()
            .execute(
                "UPDATE custom_dictionary SET updated_at = ? WHERE word != 'Learned'",
                [stale],
            )
            .unwrap();
        db.record_dictionary_usage("Busy Busy Learned Gone")
            .unwrap();
        db.apply_dictionary_changes(NONE, &["Gone"], manual())
            .unwrap();
        assert_eq!(
            db.get_dictionary_by_priority().unwrap(),
            ["Busy", "Stale", "Learned"]
        );
    }

    fn correction(misheard: &str, term: &str) -> Correction {
        Correction {
            misheard: misheard.into(),
            term: term.into(),
        }
    }

    #[test]
    fn a_strong_correction_adds_its_term_at_once() {
        let db = db();
        let added = db
            .record_corrections(&[correction("Shunade", "Sinead")])
            .unwrap();
        assert_eq!(added, ["Sinead"]);
        assert_eq!(
            db.dictionary_word_source("Sinead").unwrap().as_deref(),
            Some("learned")
        );
        assert_eq!(
            db.get_known_corrections().unwrap(),
            [KnownCorrection {
                misheard: "Shunade".into(),
                term: "Sinead".into(),
                count: 1,
            }]
        );
    }

    #[test]
    fn a_context_dependent_correction_is_remembered_on_the_first_edit() {
        let db = db();
        let outcome = db
            .record_session_corrections_if_current(
                "edit",
                1,
                &[correction("cloud", "Claude")],
                &AtomicU64::new(0),
                0,
            )
            .unwrap();
        assert_eq!(outcome.context_dependent_terms, ["Claude"]);
        assert_eq!(db.get_known_corrections().unwrap()[0].count, 1);
        assert!(!db.get_known_corrections().unwrap()[0].applies_automatically());
    }

    #[test]
    fn repeated_snapshots_of_one_session_count_as_one_correction() {
        let db = db();
        let generation = AtomicU64::new(1);
        let corrections = [correction("cloud", "Claude")];
        db.record_session_corrections_if_current("field-session", 1, &corrections, &generation, 1)
            .unwrap();
        let repeated = db
            .record_session_corrections_if_current("field-session", 2, &corrections, &generation, 1)
            .unwrap();
        assert_eq!(repeated, CorrectionLearningOutcome::default());
        assert_eq!(db.get_known_corrections().unwrap()[0].count, 1);
    }

    #[test]
    fn a_new_clear_correction_reports_success_without_readding_a_manual_word() {
        let db = db();
        db.apply_dictionary_changes(&["Claude Code"], NONE, manual())
            .unwrap();
        let generation = AtomicU64::new(1);
        let corrections = [correction("cloud code", "Claude Code")];
        let outcome = db
            .record_session_corrections_if_current("session", 1, &corrections, &generation, 1)
            .unwrap();
        assert!(outcome.added_terms.is_empty());
        assert_eq!(outcome.clear_correction_pairs, corrections);
        assert_eq!(
            db.dictionary_word_source("Claude Code").unwrap().as_deref(),
            Some("manual")
        );
    }

    #[test]
    fn the_latest_session_spelling_wins_without_adding_a_sighting() {
        let db = db();
        let generation = AtomicU64::new(1);
        db.record_session_corrections_if_current(
            "session",
            1,
            &[correction("github", "GitHub")],
            &generation,
            1,
        )
        .unwrap();
        db.record_session_corrections_if_current(
            "session",
            2,
            &[correction("github", "GITHUB")],
            &generation,
            1,
        )
        .unwrap();
        assert_eq!(
            db.get_known_corrections().unwrap(),
            [KnownCorrection {
                misheard: "github".into(),
                term: "GITHUB".into(),
                count: 1,
            }]
        );
    }

    #[test]
    fn the_latest_session_edit_replaces_only_its_own_learning() {
        let db = db();
        let generation = AtomicU64::new(1);
        db.apply_dictionary_changes(&["Shared"], NONE, manual())
            .unwrap();
        db.record_corrections(&[correction("historic", "Historical")])
            .unwrap();
        db.record_session_corrections_if_current(
            "first",
            1,
            &[
                correction("cloud", "Claude"),
                correction("Shunade", "Sinead"),
                correction("shared", "Shared"),
                correction("historic", "Historical"),
            ],
            &generation,
            1,
        )
        .unwrap();
        db.record_session_corrections_if_current(
            "second",
            1,
            &[correction("cloud", "Claude")],
            &generation,
            1,
        )
        .unwrap();
        db.record_session_corrections_if_current(
            "first",
            2,
            &[correction("cloud", "Claudette")],
            &generation,
            1,
        )
        .unwrap();
        let mut words = db.get_dictionary().unwrap();
        words.sort();
        assert_eq!(words, ["Claude", "Claudette", "Historical", "Shared"]);
        let mut pairs: Vec<_> = db
            .get_known_corrections()
            .unwrap()
            .into_iter()
            .map(|c| (c.misheard, c.term, c.count))
            .collect();
        pairs.sort();
        assert_eq!(
            pairs,
            [
                ("cloud".into(), "Claude".into(), 1),
                ("cloud".into(), "Claudette".into(), 1),
                ("historic".into(), "Historical".into(), 1),
            ]
        );
    }

    #[test]
    fn clearing_learned_data_allows_a_current_session_to_start_fresh() {
        let db = db();
        let generation = AtomicU64::new(1);
        let corrections = [correction("cloud code", "Claude Code")];
        db.record_session_corrections_if_current("session", 1, &corrections, &generation, 1)
            .unwrap();
        generation.store(2, Ordering::Release);
        db.clear_dictionary().unwrap();
        assert!(
            db.record_session_corrections_if_current("session", 2, &corrections, &generation, 1)
                .unwrap()
                .added_terms
                .is_empty()
        );
        assert_eq!(
            db.record_session_corrections_if_current("session", 3, &corrections, &generation, 2)
                .unwrap()
                .added_terms,
            ["Claude Code"]
        );
    }

    #[test]
    fn revising_a_session_after_a_unicode_respelling_retracts_its_correction() {
        let db = db();
        let generation = AtomicU64::new(1);
        db.record_session_corrections_if_current(
            "session",
            1,
            &[correction("eclair", "Éclair")],
            &generation,
            1,
        )
        .unwrap();
        db.apply_dictionary_changes(&["éclair"], &["Éclair"], manual())
            .unwrap();
        db.record_session_corrections_if_current("session", 2, &[], &generation, 1)
            .unwrap();
        assert!(db.get_known_corrections().unwrap().is_empty());
        assert_eq!(db.get_dictionary().unwrap(), ["éclair"]);
    }

    #[test]
    fn a_removed_word_does_not_let_an_old_session_erase_new_learning() {
        let db = db();
        let generation = AtomicU64::new(1);
        let corrections = [correction("cloud", "Claude")];
        db.record_session_corrections_if_current("old", 1, &corrections, &generation, 1)
            .unwrap();
        db.apply_dictionary_changes(NONE, &["Claude"], manual())
            .unwrap();
        db.apply_dictionary_changes(&["Claude"], NONE, manual())
            .unwrap();
        db.record_session_corrections_if_current("new", 1, &corrections, &generation, 1)
            .unwrap();
        db.record_session_corrections_if_current("old", 2, &[], &generation, 1)
            .unwrap();
        assert_eq!(
            db.get_known_corrections().unwrap(),
            [KnownCorrection {
                misheard: "cloud".into(),
                term: "Claude".into(),
                count: 1,
            }]
        );
    }

    #[test]
    fn forgetting_a_correction_detaches_its_old_session_contributions() {
        let db = db();
        let generation = AtomicU64::new(1);
        let corrections = [correction("cloud", "Claude")];
        db.record_session_corrections_if_current("old", 1, &corrections, &generation, 1)
            .unwrap();
        db.remove_correction("cloud", "Claude").unwrap();
        db.record_session_corrections_if_current("new", 1, &corrections, &generation, 1)
            .unwrap();
        db.record_session_corrections_if_current("old", 2, &[], &generation, 1)
            .unwrap();
        assert_eq!(db.get_known_corrections().unwrap()[0].count, 1);
    }

    #[test]
    fn an_invalidated_learning_result_cannot_restore_a_cleared_dictionary() {
        let db = db();
        let generation = AtomicU64::new(7);
        let learn = correction("Shunade", "Sinead");
        assert_eq!(
            db.record_session_corrections_if_current(
                "edit",
                1,
                std::slice::from_ref(&learn),
                &generation,
                7
            )
            .unwrap()
            .added_terms,
            ["Sinead"]
        );
        generation.fetch_add(1, Ordering::AcqRel);
        db.clear_dictionary().unwrap();
        assert!(
            db.record_session_corrections_if_current("edit", 2, &[learn], &generation, 7)
                .unwrap()
                .added_terms
                .is_empty()
        );
        assert!(db.get_dictionary().unwrap().is_empty());
        let stored_corrections: i64 = db
            .lock()
            .query_row("SELECT COUNT(*) FROM dictionary_corrections", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(stored_corrections, 0);
    }

    #[test]
    fn a_context_dependent_term_is_added_without_deliberate_repetition() {
        let db = db();
        let weak = correction("affect", "effect");
        assert_eq!(
            db.record_corrections(std::slice::from_ref(&weak)).unwrap(),
            ["effect"]
        );
        assert_eq!(db.get_dictionary().unwrap(), ["effect"]);
        assert!(!db.get_known_corrections().unwrap()[0].applies_automatically());
        assert!(db.record_corrections(&[weak]).unwrap().is_empty());
    }

    #[test]
    fn different_mishearings_of_a_term_add_up() {
        let db = db();
        assert_eq!(
            db.record_corrections(&[correction("克劳", "Claude")])
                .unwrap(),
            ["Claude"]
        );
        assert!(
            db.record_corrections(&[correction("cloud", "Claude")])
                .unwrap()
                .is_empty()
        );
        assert_eq!(db.get_known_corrections().unwrap().len(), 2);
    }

    #[test]
    fn repeating_a_correction_counts_for_terms_already_in_the_dictionary() {
        let db = db();
        db.apply_dictionary_changes(&["Claude"], NONE, manual())
            .unwrap();
        for _ in 0..2 {
            assert!(
                db.record_corrections(&[correction("cloud", "Claude")])
                    .unwrap()
                    .is_empty()
            );
        }
        assert_eq!(db.get_known_corrections().unwrap()[0].count, 2);
        assert_eq!(
            db.dictionary_word_source("Claude").unwrap().as_deref(),
            Some("manual")
        );
    }

    #[test]
    fn one_batch_can_name_a_term_twice() {
        let db = db();
        let added = db
            .record_corrections(&[
                correction("cloud", "Claude"),
                correction("克劳德", "Claude"),
            ])
            .unwrap();
        assert_eq!(added, ["Claude"]);
        assert_eq!(db.get_known_corrections().unwrap().len(), 2);
    }

    #[test]
    fn a_capitalization_fix_respells_the_stored_word() {
        let db = db();
        db.apply_dictionary_changes(&["github"], NONE, manual())
            .unwrap();
        db.record_corrections(&[correction("github", "GitHub")])
            .unwrap();
        assert_eq!(db.get_dictionary().unwrap(), ["GitHub"]);

        db.record_corrections(&[correction("gethub", "github")])
            .unwrap();
        assert_eq!(
            db.get_dictionary().unwrap(),
            ["GitHub"],
            "a correction from another spelling leaves it alone"
        );
    }

    #[test]
    fn unicode_case_rename_keeps_corrections_joined_with_their_counts() {
        let db = db();
        let correction = correction("eclair", "Éclair");
        db.record_corrections(std::slice::from_ref(&correction))
            .unwrap();
        db.record_corrections(&[correction]).unwrap();
        db.apply_dictionary_changes(&["éclair"], &["Éclair"], manual())
            .unwrap();
        assert_eq!(db.get_dictionary().unwrap(), ["éclair"]);
        let known = db.get_known_corrections().unwrap();
        assert_eq!(known.len(), 1);
        assert_eq!(known[0].misheard, "eclair");
        assert_eq!(known[0].term, "éclair");
        assert_eq!(known[0].count, 2);
    }

    #[test]
    fn successive_unicode_case_corrections_follow_the_latest_spelling() {
        let db = db();
        db.record_corrections(&[correction("eclair", "Éclair")])
            .unwrap();
        db.record_corrections(&[
            correction("Éclair", "éclair"),
            correction("éclair", "ÉCLAIR"),
        ])
        .unwrap();
        assert_eq!(db.get_dictionary().unwrap(), ["ÉCLAIR"]);
        let known = db.get_known_corrections().unwrap();
        assert!(
            known
                .iter()
                .any(|c| c.misheard == "eclair" && c.term == "ÉCLAIR")
        );
        assert!(known.iter().all(|c| c.term == "ÉCLAIR"));
    }

    #[test]
    fn unicode_case_rename_rolls_back_if_a_reference_conflicts() {
        let db = db();
        db.record_corrections(&[correction("eclair", "Éclair")])
            .unwrap();
        // Older data can contain both spellings under SQLite's ASCII collation.
        db.lock()
            .execute(
                "INSERT INTO dictionary_corrections (misheard, term) VALUES ('eclair', 'éclair')",
                [],
            )
            .unwrap();
        assert!(
            db.apply_dictionary_changes(&["éclair"], &["Éclair"], manual())
                .is_err()
        );
        assert_eq!(db.get_dictionary().unwrap(), ["Éclair"]);
        assert_eq!(
            db.dictionary_word_source("Éclair").unwrap().as_deref(),
            Some("learned")
        );
        let count: i64 = db
            .lock()
            .query_row("SELECT COUNT(*) FROM dictionary_corrections", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 2);
        assert_eq!(db.get_known_corrections().unwrap()[0].term, "Éclair");
    }

    #[test]
    fn unicode_rejected_terms_stay_removed_until_manually_added() {
        let db = db();
        db.record_corrections(&[correction("eclair", "Éclair")])
            .unwrap();
        db.apply_dictionary_changes(NONE, &["Éclair"], manual())
            .unwrap();
        for _ in 0..2 {
            assert!(
                db.record_corrections(&[correction("eclair", "éclair")])
                    .unwrap()
                    .is_empty()
            );
        }
        assert!(db.get_dictionary().unwrap().is_empty());
        let count: i64 = db
            .lock()
            .query_row("SELECT COUNT(*) FROM dictionary_corrections", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, 0);
        db.apply_dictionary_changes(&["éclair"], NONE, manual())
            .unwrap();
        db.record_corrections(&[correction("eclair", "éclair")])
            .unwrap();
        assert_eq!(db.get_known_corrections().unwrap().len(), 1);
    }

    #[test]
    fn opposite_contexts_keep_independent_session_contributions() {
        for removed in ["first", "second"] {
            let db = db();
            db.apply_dictionary_changes(&["Alpha", "Beta"], NONE, manual())
                .unwrap();
            let generation = AtomicU64::new(1);
            db.record_session_corrections_if_current(
                "first",
                1,
                &[correction("Alpha", "Beta")],
                &generation,
                1,
            )
            .unwrap();
            db.record_session_corrections_if_current(
                "second",
                1,
                &[correction("Beta", "Alpha")],
                &generation,
                1,
            )
            .unwrap();
            db.record_session_corrections_if_current(removed, 2, &[], &generation, 1)
                .unwrap();
            let (misheard, term) = if removed == "first" {
                ("Beta", "Alpha")
            } else {
                ("Alpha", "Beta")
            };
            db.apply_dictionary_changes(NONE, &[misheard], manual())
                .unwrap();
            assert_eq!(
                db.get_known_corrections().unwrap(),
                [KnownCorrection {
                    misheard: misheard.into(),
                    term: term.into(),
                    count: 1
                }]
            );
        }
    }

    #[test]
    fn a_removed_word_is_not_learned_again_until_added_by_hand() {
        let db = db();
        let learn = correction("Shunade", "Sinead");
        db.record_corrections(std::slice::from_ref(&learn)).unwrap();
        db.apply_dictionary_changes(NONE, &["sinead"], manual())
            .unwrap();
        assert!(db.get_known_corrections().unwrap().is_empty());

        assert!(
            db.record_corrections(std::slice::from_ref(&learn))
                .unwrap()
                .is_empty()
        );
        assert!(db.get_dictionary().unwrap().is_empty());

        db.apply_dictionary_changes(&["Sinead"], NONE, manual())
            .unwrap();
        db.record_corrections(&[learn]).unwrap();
        assert_eq!(db.get_known_corrections().unwrap().len(), 1);
    }

    #[test]
    fn clearing_the_dictionary_is_a_fresh_start() {
        let db = db();
        let learn = correction("Shunade", "Sinead");
        db.record_corrections(std::slice::from_ref(&learn)).unwrap();
        db.apply_dictionary_changes(NONE, &["Sinead"], manual())
            .unwrap();
        db.clear_dictionary().unwrap();
        assert_eq!(db.record_corrections(&[learn]).unwrap(), ["Sinead"]);
    }

    #[test]
    fn a_removed_mishearing_is_forgotten_and_its_word_stays() {
        let db = db();
        db.record_corrections(&[correction("Shunade", "Sinead")])
            .unwrap();
        db.remove_correction("shunade", "SINEAD").unwrap();
        assert!(db.get_known_corrections().unwrap().is_empty());
        assert_eq!(db.get_dictionary().unwrap(), ["Sinead"]);
    }

    #[test]
    fn a_mishearing_that_is_a_dictionary_word_is_not_applied() {
        let db = db();
        db.record_corrections(&[correction("cloud", "Claude")])
            .unwrap();
        db.apply_dictionary_changes(&["Cloud"], NONE, manual())
            .unwrap();
        assert!(db.get_known_corrections().unwrap().is_empty());
    }

    // Priority ranking

    const NOW: &str = "2026-10-12T12:00:00Z";

    fn now_ms() -> i64 {
        DateTime::parse_from_rfc3339(NOW)
            .unwrap()
            .timestamp_millis()
    }

    fn days_ago(days: f64) -> String {
        let ms = now_ms() - (days * DAY_MS as f64) as i64;
        format_db_timestamp(&DateTime::from_timestamp_millis(ms).unwrap())
    }

    fn rank_row(id: i64, word: &str, source: &str, uses: i64) -> DictionaryRankRow {
        DictionaryRankRow {
            id,
            word: word.into(),
            source: source.into(),
            use_count: uses,
            ..Default::default()
        }
    }

    #[test]
    fn ranks_manual_before_learned() {
        let rows = vec![
            DictionaryRankRow {
                last_used_at: Some(days_ago(0.0)),
                ..rank_row(1, "learned", "learned", 50)
            },
            DictionaryRankRow {
                updated_at: days_ago(90.0),
                ..rank_row(2, "manual", "manual", 0)
            },
        ];
        assert_eq!(rank_dictionary_rows(rows, now_ms()), ["manual", "learned"]);
    }

    #[test]
    fn ranks_by_freshness_then_use_then_newest() {
        let rows = vec![
            DictionaryRankRow {
                last_used_at: Some(days_ago(30.0)),
                ..rank_row(1, "old-busy", "manual", 40)
            },
            DictionaryRankRow {
                last_used_at: Some(days_ago(7.0)),
                ..rank_row(2, "week", "manual", 1)
            },
            DictionaryRankRow {
                last_used_at: Some(days_ago(0.5)),
                ..rank_row(3, "today", "manual", 1)
            },
            DictionaryRankRow {
                last_used_at: Some(days_ago(10.0)),
                ..rank_row(4, "week-busy", "manual", 5)
            },
            DictionaryRankRow {
                last_used_at: Some(days_ago(3.0)),
                ..rank_row(5, "week-new", "manual", 1)
            },
        ];
        assert_eq!(
            rank_dictionary_rows(rows, now_ms()),
            ["today", "week-busy", "week-new", "week", "old-busy"]
        );
    }

    #[test]
    fn an_unused_word_is_as_fresh_as_its_last_edit() {
        let rows = vec![
            DictionaryRankRow {
                last_used_at: Some(days_ago(60.0)),
                ..rank_row(1, "stale", "manual", 3)
            },
            DictionaryRankRow {
                updated_at: days_ago(0.0),
                ..rank_row(2, "just-added", "manual", 0)
            },
        ];
        assert_eq!(
            rank_dictionary_rows(rows, now_ms()),
            ["just-added", "stale"]
        );
    }
}
