//! Snippets (spoken trigger -> replacement text) storage.
//!
//! Whole-list writes are diffed against the table so unchanged rows keep
//! their id, and with it their position in the list. Triggers are unique
//! case-insensitively.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use rusqlite::{TransactionBehavior, params};

use super::Database;
use crate::analytics::trim_text;

/// Longest trigger stored, in characters; longer ones are dropped.
pub const MAX_SNIPPET_TRIGGER_LENGTH: usize = 100;

/// The text module owns the snippet type; storage reuses it.
pub use crate::text::snippets::Snippet;

impl Snippet {
    pub fn new(trigger: impl Into<String>, replacement: impl Into<String>) -> Self {
        Self {
            trigger: trigger.into(),
            replacement: replacement.into(),
        }
    }

    /// Keeps existing row order and appends new triggers, just like SQLite's
    /// diff-based whole-list write. Repeated UI edits can use this immediately.
    pub fn project_list(existing: &[Self], requested: &[Self]) -> Vec<Self> {
        let cleaned = normalized_snippets(requested);
        let mut by_key: HashMap<&str, &Snippet> = cleaned
            .iter()
            .map(|(key, snippet)| (key.as_str(), snippet))
            .collect();
        let mut projected = Vec::with_capacity(cleaned.len());
        for old in existing {
            if let Some(snippet) = by_key.remove(old.trigger.to_lowercase().as_str()) {
                projected.push(snippet.clone());
            }
        }
        for (key, snippet) in &cleaned {
            if by_key.contains_key(key.as_str()) {
                projected.push(snippet.clone());
            }
        }
        projected
    }
}

fn normalized_snippets(snippets: &[Snippet]) -> Vec<(String, Snippet)> {
    let mut seen = HashSet::new();
    let mut cleaned: Vec<(String, Snippet)> = Vec::new();
    for raw in snippets {
        let trigger = trim_text(&raw.trigger);
        let replacement = trim_text(&raw.replacement);
        if trigger.is_empty() || replacement.is_empty() {
            continue;
        }
        if trigger.chars().count() > MAX_SNIPPET_TRIGGER_LENGTH {
            continue;
        }
        let lower = trigger.to_lowercase();
        if seen.insert(lower.clone()) {
            cleaned.push((lower, Snippet::new(trigger, replacement)));
        }
    }
    cleaned
}

impl Database {
    /// Snippets in insertion order.
    pub fn get_snippets(&self) -> Result<Vec<Snippet>> {
        let conn = self.lock();
        let snippets = conn
            .prepare("SELECT trigger, replacement FROM snippets ORDER BY id ASC")?
            .query_map([], |row| {
                Ok(Snippet {
                    trigger: row.get(0)?,
                    replacement: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(snippets)
    }

    /// Applies only the user's changes from a possibly optimistic UI list.
    /// An unchanged row from an earlier failed write must not be resurrected
    /// by the next edit's whole-list snapshot.
    pub fn apply_snippet_changes(&self, before: &[Snippet], requested: &[Snippet]) -> Result<()> {
        let before = normalized_snippets(before);
        let requested = normalized_snippets(requested);
        let old: HashMap<&str, &Snippet> = before
            .iter()
            .map(|(key, snippet)| (key.as_str(), snippet))
            .collect();
        let next: HashSet<&str> = requested.iter().map(|(key, _)| key.as_str()).collect();
        let mut actual = self.get_snippets()?;
        actual.retain(|snippet| {
            let key = snippet.trigger.to_lowercase();
            !old.contains_key(key.as_str()) || next.contains(key.as_str())
        });
        let positions: HashMap<String, usize> = actual
            .iter()
            .enumerate()
            .map(|(index, snippet)| (snippet.trigger.to_lowercase(), index))
            .collect();
        for (key, snippet) in requested {
            if old
                .get(key.as_str())
                .is_some_and(|previous| **previous == snippet)
            {
                continue;
            }
            if let Some(&index) = positions.get(&key) {
                actual[index] = snippet;
            } else {
                actual.push(snippet);
            }
        }
        self.set_snippets(&actual)
    }

    /// Replaces the snippet list. Triggers and replacements are trimmed,
    /// empty or over-long entries dropped, and case-variant triggers deduped
    /// (first wins).
    pub fn set_snippets(&self, snippets: &[Snippet]) -> Result<()> {
        let cleaned = normalized_snippets(snippets);
        let seen: HashSet<&str> = cleaned.iter().map(|(key, _)| key.as_str()).collect();

        let mut conn = self.lock();
        let existing: HashMap<String, i64> = conn
            .prepare("SELECT id, trigger FROM snippets")?
            .query_map([], |row| {
                Ok((row.get::<_, String>(1)?.to_lowercase(), row.get(0)?))
            })?
            .collect::<rusqlite::Result<_>>()?;

        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        {
            let mut delete = tx.prepare("DELETE FROM snippets WHERE id = ?")?;
            let mut update = tx.prepare(
                "UPDATE snippets SET trigger = ?, replacement = ?
                 WHERE id = ? AND (trigger != ? COLLATE BINARY OR replacement != ?)",
            )?;
            let mut insert =
                tx.prepare("INSERT INTO snippets (trigger, replacement) VALUES (?, ?)")?;

            for (lower, id) in &existing {
                if !seen.contains(lower.as_str()) {
                    delete.execute([id])?;
                }
            }
            for (lower, snippet) in &cleaned {
                match existing.get(lower) {
                    Some(id) => {
                        update.execute(params![
                            snippet.trigger,
                            snippet.replacement,
                            id,
                            snippet.trigger,
                            snippet.replacement
                        ])?;
                    }
                    None => {
                        insert.execute(params![snippet.trigger, snippet.replacement])?;
                    }
                }
            }
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_ids(db: &Database) -> Vec<i64> {
        db.lock()
            .prepare("SELECT id FROM snippets ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn immediate_lists_and_delta_writes_match_whole_list_storage() {
        let db = Database::open_in_memory().unwrap();
        let reference = Database::open_in_memory().unwrap();
        let mut projected = Vec::new();
        let requests = [
            vec![
                Snippet::new(" One ", " first "),
                Snippet::new("two", "2"),
                Snippet::new("É", "unicode"),
            ],
            vec![
                Snippet::new("TWO", "updated"),
                Snippet::new("one", "first"),
                Snippet::new("three", "3"),
            ],
            vec![
                Snippet::new("Three", "third"),
                Snippet::new("THREE", "duplicate"),
                Snippet::new("", "empty"),
                Snippet::new("x".repeat(101), "long"),
            ],
            vec![
                Snippet::new("one", "back"),
                Snippet::new("three", "third"),
                Snippet::new("i̇", "unicode"),
            ],
            vec![],
        ];
        for requested in requests {
            db.apply_snippet_changes(&projected, &requested).unwrap();
            reference.set_snippets(&requested).unwrap();
            projected = Snippet::project_list(&projected, &requested);
            assert_eq!(projected, reference.get_snippets().unwrap());
            assert_eq!(db.get_snippets().unwrap(), projected);
        }
    }

    #[test]
    fn a_later_snippet_edit_does_not_resurrect_an_earlier_failed_insert() {
        let db = Database::open_in_memory().unwrap();
        db.lock().execute_batch("CREATE TRIGGER reject_first BEFORE INSERT ON snippets WHEN NEW.trigger = 'first' BEGIN SELECT RAISE(ABORT, 'synthetic write failure'); END;").unwrap();
        let first = vec![Snippet::new("first", "1")];
        assert!(db.apply_snippet_changes(&[], &first).is_err());
        db.lock()
            .execute_batch("DROP TRIGGER reject_first;")
            .unwrap();
        let mut second = first.clone();
        second.push(Snippet::new("second", "2"));
        db.apply_snippet_changes(&first, &second).unwrap();
        assert_eq!(
            db.get_snippets().unwrap(),
            vec![Snippet::new("second", "2")]
        );
    }

    #[test]
    fn trims_dedupes_preserves_no_ops_and_updates_replacements() {
        let db = Database::open_in_memory().unwrap();
        db.set_snippets(&[
            Snippet::new("  signoff  ", "  Regards  "),
            Snippet::new("SIGNOFF", "Ignored duplicate"),
        ])
        .unwrap();
        assert_eq!(
            db.get_snippets().unwrap(),
            [Snippet::new("signoff", "Regards")]
        );
        let ids = row_ids(&db);

        db.set_snippets(&[Snippet::new("signoff", "Regards")])
            .unwrap();
        assert_eq!(
            db.get_snippets().unwrap(),
            [Snippet::new("signoff", "Regards")]
        );
        assert_eq!(row_ids(&db), ids, "an unchanged snippet keeps its row");

        db.set_snippets(&[Snippet::new("SignOff", "Regards")])
            .unwrap();
        assert_eq!(
            db.get_snippets().unwrap(),
            [Snippet::new("SignOff", "Regards")],
            "a case-only trigger change is saved"
        );
        db.set_snippets(&[Snippet::new("SignOff", "Best regards")])
            .unwrap();
        assert_eq!(
            db.get_snippets().unwrap(),
            [Snippet::new("SignOff", "Best regards")]
        );
        assert_eq!(row_ids(&db), ids);
    }

    #[test]
    fn removals_delete_rows_and_keep_the_rest_in_order() {
        let db = Database::open_in_memory().unwrap();
        db.set_snippets(&[
            Snippet::new("one", "1"),
            Snippet::new("temp", "Temporary"),
            Snippet::new("two", "2"),
        ])
        .unwrap();
        db.set_snippets(&[Snippet::new("two", "2"), Snippet::new("one", "1")])
            .unwrap();
        assert_eq!(
            db.get_snippets().unwrap(),
            [Snippet::new("one", "1"), Snippet::new("two", "2")]
        );
        db.set_snippets(&[]).unwrap();
        assert!(row_ids(&db).is_empty());
        db.set_snippets(&[Snippet::new("TEMP", "Back")]).unwrap();
        assert_eq!(db.get_snippets().unwrap(), [Snippet::new("TEMP", "Back")]);
    }

    #[test]
    fn drops_triggers_longer_than_the_limit() {
        let db = Database::open_in_memory().unwrap();
        db.set_snippets(&[
            Snippet::new("x".repeat(101), "too long"),
            Snippet::new("ok", "fine"),
        ])
        .unwrap();
        assert_eq!(db.get_snippets().unwrap(), [Snippet::new("ok", "fine")]);
    }
}
