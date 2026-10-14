//! Insights counters (`analytics_events`).
//!
//! Counters are content-free: one row per completed dictation with its word
//! count and spoken duration, recorded when the dictation is delivered. They
//! are independent of History rows, so deleting a transcription keeps its
//! counter; Clear History and the retention sweep remove both.

use anyhow::Result;
use rusqlite::{Connection, params};

use super::Database;
use crate::analytics::{
    AnalyticsDayTotals, AnalyticsEventInput, AnalyticsSummary, summarize_analytics_days, today_key,
};

/// Retention for counters, on the transcript cutoff. Returns how many
/// counters were removed.
pub(super) fn purge_before(conn: &Connection, cutoff: &str) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM analytics_events WHERE occurred_at < ?",
        [cutoff],
    )?)
}

impl Database {
    /// Records one dictation's counter. Returns false when it was ignored
    /// because it has no words.
    pub fn record_analytics_event(&self, input: &AnalyticsEventInput) -> Result<bool> {
        if input.word_count == 0 {
            return Ok(false);
        }
        let spoken = input.spoken_duration_ms.filter(|ms| *ms > 0);
        self.lock().execute(
            "INSERT INTO analytics_events (
               occurred_at, local_date, word_count, spoken_duration_ms, provider, model
             ) VALUES (?, ?, ?, ?, ?, ?)",
            params![
                input.occurred_at,
                input.local_date,
                input.word_count,
                spoken,
                input.provider,
                input.model
            ],
        )?;
        Ok(true)
    }

    /// The Insights summary, with today taken from the local clock.
    pub fn get_analytics_summary(&self) -> Result<AnalyticsSummary> {
        self.get_analytics_summary_for(&today_key())
    }

    /// The Insights summary as of the local date `today` (`YYYY-MM-DD`).
    pub fn get_analytics_summary_for(&self, today: &str) -> Result<AnalyticsSummary> {
        let conn = self.lock();
        // Grouped in SQL so the row count is bounded by distinct days.
        let days = conn
            .prepare(
                "SELECT local_date,
                        SUM(word_count),
                        COUNT(*),
                        SUM(COALESCE(spoken_duration_ms, 0)),
                        SUM(CASE WHEN spoken_duration_ms IS NOT NULL THEN word_count ELSE 0 END)
                 FROM analytics_events
                 GROUP BY local_date",
            )?
            .query_map([], |row| {
                Ok(AnalyticsDayTotals {
                    date: row.get(0)?,
                    words: row.get(1)?,
                    dictations: row.get(2)?,
                    spoken_duration_ms: row.get(3)?,
                    covered_words: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(summarize_analytics_days(days, today))
    }
}
