//! Ordered publication of dictionary reads from the serial storage worker.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::db::{Database, DictionaryEntry};
use crate::text::correction_learner::KnownCorrection;

static NEXT_SNAPSHOT: AtomicU64 = AtomicU64::new(1);

pub(super) struct DictionarySnapshot {
    sequence: u64,
    entries: anyhow::Result<Vec<DictionaryEntry>>,
    corrections: anyhow::Result<Vec<KnownCorrection>>,
}

impl DictionarySnapshot {
    /// Call on the storage worker so sequence order follows database reads,
    /// independently of the order in which UI futures resume.
    pub(super) fn read(db: &Database) -> Self {
        let entries = db.get_dictionary_entries();
        let corrections = db.get_known_corrections();
        Self {
            sequence: NEXT_SNAPSHOT.fetch_add(1, Ordering::SeqCst),
            entries,
            corrections,
        }
    }

    /// Manual editing historically reloads each failed query as an empty list.
    pub(super) fn default_on_read_failure(mut self) -> Self {
        self.entries = Ok(self.entries.unwrap_or_default());
        self.corrections = Ok(self.corrections.unwrap_or_default());
        self
    }

    pub(super) fn apply(
        self,
        last_sequence: &mut u64,
        entries: &mut Vec<DictionaryEntry>,
        corrections: &mut Vec<KnownCorrection>,
    ) -> bool {
        if self.sequence <= *last_sequence {
            return false;
        }
        let (Ok(next_entries), Ok(next_corrections)) = (self.entries, self.corrections) else {
            return false;
        };
        *last_sequence = self.sequence;
        *entries = next_entries;
        *corrections = next_corrections;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::DictionarySource;

    #[test]
    fn reversed_completions_cannot_overwrite_newer_usage_or_corrections() {
        let db = Database::open_in_memory().unwrap();
        db.apply_dictionary_changes(&["Whisper"], &[] as &[&str], DictionarySource::Manual)
            .unwrap();
        let older = DictionarySnapshot::read(&db);
        db.record_dictionary_usage("Whisper").unwrap();
        db.record_corrections(&[crate::text::correction_learner::Correction {
            misheard: "whispr".into(),
            term: "Whisper".into(),
            needs_repeat: false,
        }])
        .unwrap();
        let newer = DictionarySnapshot::read(&db);
        let (mut sequence, mut entries, mut corrections) = (0, Vec::new(), Vec::new());
        assert!(newer.apply(&mut sequence, &mut entries, &mut corrections));
        assert!(!older.apply(&mut sequence, &mut entries, &mut corrections));
        assert_eq!(entries, db.get_dictionary_entries().unwrap());
        assert_eq!(corrections, db.get_known_corrections().unwrap());
        assert_eq!(entries[0].use_count, 1);
        assert!(!corrections.is_empty());
    }

    #[test]
    fn a_failed_dictionary_write_reconciles_the_immediate_projection() {
        let db = Database::open_in_memory().unwrap();
        db.apply_dictionary_changes(&["saved"], &[] as &[&str], DictionarySource::Manual)
            .unwrap();
        let mut entries = db.get_dictionary_entries().unwrap();
        DictionaryEntry::project_changes(
            &mut entries,
            &["rejected"],
            &["saved"],
            DictionarySource::Manual,
        );
        db.lock().execute_batch("CREATE TRIGGER reject_edit BEFORE INSERT ON custom_dictionary BEGIN SELECT RAISE(ABORT, 'synthetic write failure'); END;").unwrap();
        assert!(
            db.apply_dictionary_changes(&["rejected"], &["saved"], DictionarySource::Manual)
                .is_err()
        );
        let (mut sequence, mut corrections) = (0, Vec::new());
        assert!(
            DictionarySnapshot::read(&db)
                .default_on_read_failure()
                .apply(&mut sequence, &mut entries, &mut corrections)
        );
        assert_eq!(entries, db.get_dictionary_entries().unwrap());
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.word.as_str())
                .collect::<Vec<_>>(),
            vec!["saved"]
        );
    }
}
