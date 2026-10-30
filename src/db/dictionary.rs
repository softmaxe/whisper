//! Custom dictionary storage and STT-prompt priority.
//!
//! Every edit goes through one delta path ([`Database::apply_dictionary_changes`]),
//! which touches only the words it names. Words are unique case-insensitively.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use anyhow::Result;
use chrono::{DateTime, Utc};
use regex::Regex;
use rusqlite::{Connection, Transaction, TransactionBehavior, params};
use unicode_normalization::UnicodeNormalization;

use super::Database;
use super::timestamp::{now_db_timestamp, parse_db_timestamp};
use crate::analytics::trim_text;

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

/// How many words a delta actually changed (not how many were requested).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DictionaryChangeCounts {
    pub added: usize,
    pub removed: usize,
}

const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// Freshness tiers by last use: within a day, within two weeks, older.
const FRESHNESS_TIER_LIMITS_MS: [i64; 2] = [DAY_MS, 14 * DAY_MS];

/// Scripts written without spaces, where a term sits inside a longer run of
/// letters and a word-boundary match would never find it.
static UNSPACED_SCRIPT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}]").expect("valid regex")
});

/// Case-insensitive occurrences of each dictionary word in `text`, keyed by
/// the word as given and in input order; words that do not occur are omitted.
pub fn count_dictionary_word_uses<S: AsRef<str>>(words: &[S], text: &str) -> Vec<(String, usize)> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    if text.is_empty() {
        return counts;
    }
    let haystack = text.nfc().collect::<String>().to_lowercase();
    for word in words {
        let word = word.as_ref();
        let needle = trim_text(word).nfc().collect::<String>().to_lowercase();
        if needle.is_empty() {
            continue;
        }
        let escaped = fancy_regex::escape(&needle);
        let pattern = if UNSPACED_SCRIPT.is_match(&needle) {
            escaped.into_owned()
        } else {
            format!(r"(?<![\p{{L}}\p{{N}}_]){escaped}(?![\p{{L}}\p{{N}}_])")
        };
        let Ok(re) = fancy_regex::Regex::new(&pattern) else {
            continue;
        };
        let count = re.find_iter(&haystack).filter(|m| m.is_ok()).count();
        if count == 0 {
            continue;
        }
        match counts.iter_mut().find(|(w, _)| w == word) {
            Some(entry) => entry.1 = count,
            None => counts.push((word.to_string(), count)),
        }
    }
    counts
}

/// A dictionary row as the priority ranking sees it.
#[derive(Debug, Clone, Default)]
pub(crate) struct DictionaryRankRow {
    pub id: i64,
    pub word: String,
    pub source: String,
    pub use_count: i64,
    pub last_used_at: Option<String>,
    pub updated_at: String,
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
pub(crate) fn rank_dictionary_rows(rows: Vec<DictionaryRankRow>, now_ms: i64) -> Vec<String> {
    let mut keyed: Vec<(u8, usize, i64, DictionaryRankRow)> = rows
        .into_iter()
        .map(|row| {
            let learned = u8::from(row.source == "learned");
            let tier = freshness_tier(&row, now_ms);
            (learned, tier, row.use_count, row)
        })
        .collect();
    keyed.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.cmp(&b.1))
            .then(b.2.cmp(&a.2))
            .then(b.3.id.cmp(&a.3.id))
    });
    keyed.into_iter().map(|(_, _, _, row)| row.word).collect()
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

/// Returns true when the word became present, so callers report how many
/// words they actually added.
fn upsert_word(
    tx: &Transaction<'_>,
    word: &str,
    existing: Option<&ExistingRow>,
    source: &str,
) -> Result<bool> {
    let Some(existing) = existing else {
        tx.prepare_cached("INSERT INTO custom_dictionary (word, source) VALUES (?, ?)")?
            .execute(params![word, source])?;
        return Ok(true);
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
    Ok(false)
}

fn delete_row(tx: &Transaction<'_>, existing: Option<&ExistingRow>) -> Result<bool> {
    let Some(existing) = existing else {
        return Ok(false);
    };
    let deleted = tx
        .prepare_cached("DELETE FROM custom_dictionary WHERE id = ?")?
        .execute([existing.id])?;
    Ok(deleted > 0)
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
        Ok(rank_dictionary_rows(rows, Utc::now().timestamp_millis()))
    }

    /// Counts the dictionary words a delivered dictation used. Usage is not an
    /// edit, so `updated_at` stays as it was. Returns how many distinct words
    /// were used.
    pub fn record_dictionary_usage(&self, text: &str) -> Result<usize> {
        let mut conn = self.lock();
        let rows: Vec<(i64, String)> = conn
            .prepare("SELECT id, word FROM custom_dictionary")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let words: Vec<&str> = rows.iter().map(|(_, w)| w.as_str()).collect();
        let counts = count_dictionary_word_uses(&words, text);
        if counts.is_empty() {
            return Ok(0);
        }
        let now = now_db_timestamp();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        {
            let mut record = tx.prepare(
                "UPDATE custom_dictionary SET use_count = use_count + ?, last_used_at = ? WHERE id = ?",
            )?;
            for (id, word) in &rows {
                if let Some((_, count)) = counts.iter().find(|(w, _)| w == word) {
                    record.execute(params![*count as i64, now, id])?;
                }
            }
        }
        tx.commit()?;
        Ok(counts.len())
    }

    /// Adds and/or removes specific words, leaving every other row untouched.
    /// `source` tags additions. A word on both sides is kept.
    pub fn apply_dictionary_changes<A: AsRef<str>, R: AsRef<str>>(
        &self,
        add: &[A],
        remove: &[R],
        source: DictionarySource,
    ) -> Result<DictionaryChangeCounts> {
        let additions = normalize_words(add);
        let addition_keys: HashSet<&str> = additions.iter().map(|(l, _)| l.as_str()).collect();
        let removals: Vec<(String, String)> = normalize_words(remove)
            .into_iter()
            .filter(|(lower, _)| !addition_keys.contains(lower.as_str()))
            .collect();
        if additions.is_empty() && removals.is_empty() {
            return Ok(DictionaryChangeCounts::default());
        }

        let mut conn = self.lock();
        let by_lower = dictionary_rows(&conn)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut counts = DictionaryChangeCounts::default();
        for (lower, _) in &removals {
            if delete_row(&tx, by_lower.get(lower))? {
                counts.removed += 1;
            }
        }
        for (lower, word) in &additions {
            if upsert_word(&tx, word, by_lower.get(lower), source.as_str())? {
                counts.added += 1;
            }
        }
        tx.commit()?;
        Ok(counts)
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
    fn adding_words_leaves_other_rows_untouched() {
        let db = db();
        db.apply_dictionary_changes(&["Whisper", "Alice", "Bob"], NONE, manual())
            .unwrap();
        let result = db
            .apply_dictionary_changes(&["Carol"], NONE, manual())
            .unwrap();
        assert_eq!(result.added, 1);
        assert_eq!(
            db.get_dictionary().unwrap(),
            ["Whisper", "Alice", "Bob", "Carol"]
        );
    }

    #[test]
    fn a_stale_one_word_view_cannot_delete_anything() {
        let db = db();
        db.apply_dictionary_changes(
            &["Whisper", "Alice", "Bob", "Imported Term"],
            NONE,
            manual(),
        )
        .unwrap();
        db.apply_dictionary_changes(&["Whisper"], NONE, manual())
            .unwrap();
        assert_eq!(
            db.get_dictionary().unwrap(),
            ["Whisper", "Alice", "Bob", "Imported Term"]
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
    fn a_word_on_both_sides_is_kept() {
        let db = db();
        db.apply_dictionary_changes(&["Whisper", "Alice"], NONE, manual())
            .unwrap();
        let result = db
            .apply_dictionary_changes(&["Alice"], &["Alice"], manual())
            .unwrap();
        assert_eq!(result.removed, 0);
        assert!(db.get_dictionary().unwrap().contains(&"Alice".to_string()));
    }

    #[test]
    fn removing_a_word_leaves_other_learned_words_untouched() {
        let db = db();
        db.apply_dictionary_changes(&["Kubernetes", "Postgres"], NONE, DictionarySource::Learned)
            .unwrap();
        db.apply_dictionary_changes(NONE, &["Kubernetes"], manual())
            .unwrap();
        assert_eq!(
            db.dictionary_word_source("Postgres").unwrap().as_deref(),
            Some("learned")
        );
    }

    #[test]
    fn counts_report_words_that_changed() {
        let db = db();
        db.apply_dictionary_changes(&["Whisper", "Alice"], NONE, manual())
            .unwrap();
        assert_eq!(
            db.apply_dictionary_changes(&["Alice"], NONE, manual())
                .unwrap()
                .added,
            0
        );
        assert_eq!(
            db.apply_dictionary_changes(NONE, &["Nobody"], manual())
                .unwrap()
                .removed,
            0
        );
        let mixed = db
            .apply_dictionary_changes(&["Alice", "Carol"], &["Nobody", "Alice"], manual())
            .unwrap();
        assert_eq!(
            mixed,
            DictionaryChangeCounts {
                added: 1,
                removed: 0
            }
        );
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
        assert_eq!(
            db.apply_dictionary_changes(NONE, NONE, manual()).unwrap(),
            DictionaryChangeCounts::default()
        );
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
        let result = db
            .apply_dictionary_changes(&["ALICE"], NONE, manual())
            .unwrap();
        assert_eq!(result.added, 1);
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
    fn counts_whole_word_uses_case_insensitively() {
        let counts = count_dictionary_word_uses(
            &["Kubernetes", "Claude Code", "Rust"],
            "kubernetes runs Claude Code. Did KUBERNETES restart? Trusted code.",
        );
        assert_eq!(
            counts,
            [("Kubernetes".into(), 2), ("Claude Code".into(), 1)]
        );
    }

    #[test]
    fn counts_terms_in_unspaced_scripts() {
        let counts = count_dictionary_word_uses(
            &["张珊", "クロード"],
            "请把文件发给张珊，张珊会转给クロード。",
        );
        assert_eq!(counts, [("张珊".into(), 2), ("クロード".into(), 1)]);
    }

    #[test]
    fn treats_regex_characters_literally() {
        let counts = count_dictionary_word_uses(&["C++", "Node.js"], "C++ and Nodexjs");
        assert_eq!(counts, [("C++".into(), 1)]);
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
