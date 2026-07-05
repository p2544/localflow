//! Personal dictionary: user words/names/jargon that (a) bias whisper
//! decoding via initial_prompt and (b) are passed to the LLM as PROTECTED.

use anyhow::Result;
use rusqlite::Connection;

pub struct Dictionary;

impl Dictionary {
    pub fn init(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS dictionary (
                word TEXT PRIMARY KEY,
                added_at INTEGER NOT NULL DEFAULT (unixepoch())
            );",
        )?;
        Ok(())
    }

    pub fn add(conn: &Connection, word: &str) -> Result<()> {
        let w = word.trim();
        if w.is_empty() || w.len() > 64 {
            return Ok(());
        }
        conn.execute(
            "INSERT OR IGNORE INTO dictionary (word) VALUES (?1)",
            [w],
        )?;
        Ok(())
    }

    pub fn remove(conn: &Connection, word: &str) -> Result<()> {
        conn.execute("DELETE FROM dictionary WHERE word = ?1", [word.trim()])?;
        Ok(())
    }

    pub fn all(conn: &Connection) -> Result<Vec<String>> {
        let mut stmt = conn.prepare("SELECT word FROM dictionary ORDER BY word")?;
        let words = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(words)
    }

    /// Whisper initial_prompt built from dictionary words. Kept short —
    /// whisper only attends to ~224 tokens of prompt.
    pub fn initial_prompt(words: &[String]) -> String {
        if words.is_empty() {
            return String::new();
        }
        let mut joined = String::from("Glossary: ");
        let mut budget = 600usize; // chars, conservative proxy for tokens
        for (i, w) in words.iter().enumerate() {
            if w.len() + 2 > budget {
                break;
            }
            if i > 0 {
                joined.push_str(", ");
            }
            joined.push_str(w);
            budget -= w.len() + 2;
        }
        joined.push('.');
        joined
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        Dictionary::init(&c).unwrap();
        c
    }

    #[test]
    fn add_list_remove_roundtrip() {
        let c = mem_conn();
        Dictionary::add(&c, "Kanchana").unwrap();
        Dictionary::add(&c, "LocalFlow").unwrap();
        Dictionary::add(&c, "Kanchana").unwrap(); // dup ignored
        assert_eq!(Dictionary::all(&c).unwrap(), vec!["Kanchana", "LocalFlow"]);
        Dictionary::remove(&c, "Kanchana").unwrap();
        assert_eq!(Dictionary::all(&c).unwrap(), vec!["LocalFlow"]);
    }

    #[test]
    fn initial_prompt_formats_and_caps() {
        assert_eq!(Dictionary::initial_prompt(&[]), "");
        let p = Dictionary::initial_prompt(&["Kanchana".into(), "Rayong".into()]);
        assert_eq!(p, "Glossary: Kanchana, Rayong.");
        let many: Vec<String> = (0..500).map(|i| format!("word{i}")).collect();
        let p = Dictionary::initial_prompt(&many);
        assert!(p.len() < 700);
    }

    #[test]
    fn rejects_empty_and_oversized() {
        let c = mem_conn();
        Dictionary::add(&c, "   ").unwrap();
        Dictionary::add(&c, &"x".repeat(100)).unwrap();
        assert!(Dictionary::all(&c).unwrap().is_empty());
    }
}
