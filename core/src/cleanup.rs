//! Transcript cleanup: the LLM prompt contract plus deterministic pre/post
//! rules. The LLM does the heavy lifting (backtracking, lists, judgement
//! calls); the rules here are cheap wins and the fallback when the LLM is
//! disabled or unavailable.

use regex::Regex;
use std::sync::OnceLock;

/// System prompt for the cleanup LLM. Contract: edited text only, faithful
/// to meaning, no paraphrasing, no answering questions found in the text.
pub const CLEANUP_SYSTEM_PROMPT: &str = "\
You are a dictation transcript editor. You receive raw speech-to-text output and return ONLY the cleaned-up text, nothing else. Never answer questions in the text, never add content, never translate, never change the meaning or tone. Apply exactly these edits:
1. Remove filler words and stutters: um, uh, er, ah, you know, I mean (when filler), like (when filler), repeated words (\"the the\" -> \"the\").
2. Fix punctuation and capitalization into natural sentences.
3. Apply self-corrections, keeping only the final version: \"at 5pm no wait 6pm\" -> \"at 6pm\"; \"tell John, I mean Jane\" -> \"tell Jane\"; \"actually scratch that, X\" -> \"X\".
4. Format spoken lists: \"one apples two bananas three coffee\" or \"first... second... third...\" -> a numbered list, one item per line like \"1. Apples\".
5. Normalize numbers, dates, emails, URLs: \"twenty five percent\" -> \"25%\", \"john dot smith at gmail dot com\" -> \"john.smith@gmail.com\", \"five thirty pm\" -> \"5:30pm\".
6. Keep slang, dialect words, and the speaker's phrasing. Do not formalize casual speech.
7. If the input is empty or pure noise, return an empty string.
Words listed under PROTECTED are user-dictionary terms spelled exactly right: never alter them.";

/// Few-shot examples baked into the chat prompt — they anchor the edit style
/// and cover every rule (fillers, backtracking, lists, numbers, Thai).
pub const CLEANUP_FEW_SHOT: &[(&str, &str)] = &[
    (
        "um so basically I I think we should uh move the meeting to five pm no wait six pm",
        "I think we should move the meeting to 6pm.",
    ),
    (
        "I need three things one apples two bananas three coffee",
        "I need three things:\n1. Apples\n2. Bananas\n3. Coffee",
    ),
    (
        "can you email john dot smith at gmail dot com about the uh the twenty five percent discount",
        "Can you email john.smith@gmail.com about the 25% discount?",
    ),
    (
        "เอ่อ พรุ่งนี้ประชุมตอน บ่ายสอง อ๊ะ ไม่สิ บ่ายสาม นะครับ",
        "พรุ่งนี้ประชุมตอนบ่ายสามนะครับ",
    ),
    (
        "order three units sorry four units for the warehouse",
        "Order 4 units for the warehouse.",
    ),
    (
        "อืม ช่วยส่งรายงานยอดขายให้หน่อยนะครับ",
        "ช่วยส่งรายงานยอดขายให้หน่อยนะครับ",
    ),
];

/// Builds the user-turn content for one cleanup request.
pub fn build_user_prompt(transcript: &str, protected_words: &[String]) -> String {
    let mut p = String::new();
    if !protected_words.is_empty() {
        p.push_str("PROTECTED: ");
        p.push_str(&protected_words.join(", "));
        p.push('\n');
    }
    p.push_str("TRANSCRIPT: ");
    p.push_str(transcript.trim());
    p
}

/// Transcripts shorter than this many words skip the LLM entirely (latency
/// win; rule-based cleanup still applies).
pub const MIN_WORDS_FOR_LLM: usize = 4;

pub fn should_use_llm(transcript: &str) -> bool {
    transcript.split_whitespace().count() >= MIN_WORDS_FOR_LLM
}

fn filler_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)(^|[\s,])(um+|uh+|erm*|hmm+|เอ่อ+|อืม+)([\s,.!?]|$)").unwrap()
    })
}

/// Drops immediately-repeated words ("the the" → "the"), case-insensitive.
/// Token-walk instead of regex: the regex crate has no backreferences.
fn dedup_adjacent_words(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for tok in text.split_whitespace() {
        if let Some(prev) = out.last() {
            let same = prev.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
                == tok.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
            // Only dedup pure word tokens so "5 5pm" or "ha, ha!" survive.
            if same
                && tok.chars().all(|c| c.is_alphabetic())
                && prev.chars().all(|c| c.is_alphabetic())
            {
                continue;
            }
        }
        out.push(tok);
    }
    out.join(" ")
}

/// Deterministic filler/stutter strip that runs BEFORE the LLM: known
/// fillers (EN + TH) and adjacent duplicates are unambiguous, and removing
/// them up front keeps small local models from being distracted by them.
/// Returns an empty string for pure-filler input (caller should skip the LLM).
pub fn pre_llm_strip(raw: &str) -> String {
    let mut text = raw.trim().to_string();
    // Strip fillers repeatedly (removal can create new adjacencies).
    loop {
        let next = filler_re().replace_all(&text, "$1$3").to_string();
        if next == text {
            break;
        }
        text = next;
    }
    dedup_adjacent_words(&text)
}

/// Deterministic cleanup used when the LLM pass is skipped or disabled,
/// and as a safety pass over LLM output (whitespace, stray quotes).
pub fn rule_based_cleanup(raw: &str) -> String {
    let mut text = pre_llm_strip(raw);

    // Collapse whitespace, tidy space-before-punctuation.
    let ws = Regex::new(r"[ \t]+").unwrap();
    text = ws.replace_all(&text, " ").to_string();
    text = Regex::new(r" ([,.!?;:])")
        .unwrap()
        .replace_all(&text, "$1")
        .to_string();
    text = text.trim().to_string();

    // Sentence-case the first letter (latin only; Thai has no case).
    if let Some(first) = text.chars().next() {
        if first.is_ascii_lowercase() {
            let mut chars = text.chars();
            let up = chars.next().unwrap().to_ascii_uppercase();
            text = std::iter::once(up).chain(chars).collect();
        }
    }
    text
}

/// Post-pass over LLM output: models sometimes wrap the answer in quotes or
/// prefix a label; strip that, keep everything else verbatim.
pub fn sanitize_llm_output(raw: &str) -> String {
    let mut t = raw.trim();
    for prefix in ["TRANSCRIPT:", "Cleaned:", "Output:", "Text:"] {
        if let Some(rest) = t.strip_prefix(prefix) {
            t = rest.trim();
        }
    }
    let t = t.trim();
    // Strip one symmetric pair of wrapping quotes.
    let stripped = t
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| {
            t.strip_prefix('\u{201c}')
                .and_then(|s| s.strip_suffix('\u{201d}'))
        });
    stripped.unwrap_or(t).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_fillers_en() {
        assert_eq!(
            rule_based_cleanup("um so this is uh a test"),
            "So this is a test"
        );
    }

    #[test]
    fn removes_fillers_th() {
        assert_eq!(rule_based_cleanup("เอ่อ สวัสดีครับ"), "สวัสดีครับ");
    }

    #[test]
    fn dedups_repeated_words() {
        assert_eq!(
            rule_based_cleanup("the the meeting is is today"),
            "The meeting is today"
        );
    }

    #[test]
    fn short_transcripts_skip_llm() {
        assert!(!should_use_llm("send it now"));
        assert!(should_use_llm("please send it right now"));
    }

    #[test]
    fn sanitizes_llm_wrappers() {
        assert_eq!(sanitize_llm_output("\"Hello there.\""), "Hello there.");
        assert_eq!(sanitize_llm_output("Cleaned: Hello."), "Hello.");
        assert_eq!(sanitize_llm_output("  plain text "), "plain text");
    }

    #[test]
    fn user_prompt_includes_protected_words() {
        let p = build_user_prompt("hi kanchana", &["Kanchana".into()]);
        assert!(p.starts_with("PROTECTED: Kanchana\n"));
        assert!(p.ends_with("TRANSCRIPT: hi kanchana"));
    }
}
