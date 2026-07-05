//! Local dictation history (SQLite, opt-out via settings).

use anyhow::Result;
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub raw_text: String,
    pub final_text: String,
    pub language: String,
    pub app_name: String,
    pub duration_ms: i64,
    pub created_at: i64,
}

pub struct History;

impl History {
    pub fn init(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                raw_text TEXT NOT NULL,
                final_text TEXT NOT NULL,
                language TEXT NOT NULL DEFAULT '',
                app_name TEXT NOT NULL DEFAULT '',
                duration_ms INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE INDEX IF NOT EXISTS idx_history_created ON history(created_at DESC);",
        )?;
        Ok(())
    }

    pub fn add(
        conn: &Connection,
        raw: &str,
        final_text: &str,
        language: &str,
        app_name: &str,
        duration_ms: i64,
    ) -> Result<i64> {
        conn.execute(
            "INSERT INTO history (raw_text, final_text, language, app_name, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![raw, final_text, language, app_name, duration_ms],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Newest-first, optional case-insensitive substring search.
    pub fn list(conn: &Connection, query: &str, limit: usize) -> Result<Vec<HistoryEntry>> {
        let like = format!("%{}%", query.trim());
        let mut stmt = conn.prepare(
            "SELECT id, raw_text, final_text, language, app_name, duration_ms, created_at
             FROM history
             WHERE (?1 = '%%' OR final_text LIKE ?1 OR raw_text LIKE ?1)
             ORDER BY created_at DESC, id DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![like, limit as i64], |r| {
                Ok(HistoryEntry {
                    id: r.get(0)?,
                    raw_text: r.get(1)?,
                    final_text: r.get(2)?,
                    language: r.get(3)?,
                    app_name: r.get(4)?,
                    duration_ms: r.get(5)?,
                    created_at: r.get(6)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    pub fn delete(conn: &Connection, id: i64) -> Result<()> {
        conn.execute("DELETE FROM history WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn clear(conn: &Connection) -> Result<()> {
        conn.execute("DELETE FROM history", [])?;
        Ok(())
    }
}

/// Opens (creating if needed) the app database.
pub fn open_db() -> Result<Connection> {
    let path = crate::settings::Settings::app_data_dir()?.join("localflow.db");
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    History::init(&conn)?;
    crate::dictionary::Dictionary::init(&conn)?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        History::init(&c).unwrap();
        c
    }

    #[test]
    fn add_list_search_delete() {
        let c = mem();
        History::add(&c, "um hello", "Hello.", "en", "Slack", 850).unwrap();
        History::add(&c, "สวัสดี", "สวัสดีครับ", "th", "LINE", 640).unwrap();
        let all = History::list(&c, "", 50).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].final_text, "สวัสดีครับ"); // newest first

        let hits = History::list(&c, "hello", 50).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].app_name, "Slack");

        History::delete(&c, hits[0].id).unwrap();
        assert_eq!(History::list(&c, "", 50).unwrap().len(), 1);
        History::clear(&c).unwrap();
        assert!(History::list(&c, "", 50).unwrap().is_empty());
    }
}
