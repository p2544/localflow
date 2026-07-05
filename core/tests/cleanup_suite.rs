//! M2 acceptance suite: ≥30 transcript → expected-behavior pairs covering
//! fillers, backtracking, lists, numbers, Thai + English.
//!
//! Two tiers:
//! - `rule_tier_*` run always (no model needed) and pin the deterministic
//!   fallback behavior.
//! - `llm_golden_suite` runs the real cleanup LLM. It needs a downloaded
//!   model, so it is `#[ignore]` by default:
//!     LOCALFLOW_LLM_MODEL=/path/to/model.gguf cargo test -p localflow-core \
//!         --test cleanup_suite -- --ignored --nocapture
//!
//! LLM output is judged by containment/absence assertions, not exact string
//! equality — phrasing may vary slightly, meaning must not.

use localflow_core::cleanup::{rule_based_cleanup, should_use_llm};

/// (raw transcript, must_contain, must_not_contain)
const GOLDEN: &[(&str, &[&str], &[&str])] = &[
    // --- fillers & stutters (EN) ---
    ("um so basically I think we should ship on friday", &["ship", "Friday"], &["um", "basically"]),
    ("uh can you uh send me the file", &["send", "file"], &["uh"]),
    ("I I want to to go home now", &["want", "go home"], &["I I", "to to"]),
    ("you know the demo went you know pretty well", &["demo", "well"], &[]),
    ("er the meeting is hmm at noon", &["meeting", "noon"], &["er ", "hmm"]),
    // --- backtracking / self-correction (EN) ---
    ("let's meet at five pm no wait six pm", &["6"], &["5pm", "five pm"]),
    ("send it to john I mean jane", &["Jane"], &["John"]),
    ("the budget is ten thousand actually scratch that fifteen thousand", &["fifteen"], &["ten thousand", "scratch"]),
    ("we launch tuesday no actually thursday", &["Thursday"], &["Tuesday"]),
    ("order three units sorry four units", &["unit"], &["three units", "sorry"]),
    // --- spoken lists ---
    ("I need three things one apples two bananas three coffee", &["1.", "2.", "3.", "Apples", "Bananas", "Coffee"], &[]),
    ("todo first review the pr second deploy staging third email the team", &["review", "deploy", "email"], &[]),
    ("shopping list one milk two eggs three bread four butter", &["1.", "4.", "Milk", "Butter"], &[]),
    // --- numbers / dates / emails / urls ---
    ("the discount is twenty five percent", &["25%"], &["twenty five"]),
    ("call me at five thirty pm", &["5:30"], &["five thirty"]),
    ("email john dot smith at gmail dot com", &["john.smith@gmail.com"], &[" dot ", " at "]),
    ("the price is one hundred dollars", &["100"], &["one hundred"]),
    ("visit example dot com slash docs", &["example.com/docs"], &[]),
    ("we grew revenue by fifteen percent this quarter", &["15%"], &["fifteen percent"]),
    // --- punctuation & casing ---
    ("hey are you coming to dinner tonight", &["?"], &[]),
    ("wow that is amazing news", &["Wow", "amazing news"], &[]),
    ("i will send the report tomorrow morning", &["I will", "tomorrow morning."], &[]),
    // --- tone preservation (must NOT formalize) ---
    ("yeah that's gonna be super annoying to fix", &["gonna", "annoying"], &[]),
    ("nah dude the api is totally busted", &["dude", "busted"], &[]),
    // --- meaning preservation (must NOT answer questions) ---
    ("what time does the store open on sunday", &["store", "Sunday", "?"], &["The store opens", "9", "10am"]),
    ("do you think we should refactor this module", &["refactor", "?"], &[]),
    // --- Thai: fillers ---
    ("เอ่อ พรุ่งนี้ขอเลื่อนประชุมเป็นบ่ายนะครับ", &["พรุ่งนี้", "ประชุม", "บ่าย"], &["เอ่อ"]),
    ("อืม ช่วยส่งไฟล์ให้หน่อยได้ไหมครับ", &["ส่งไฟล์"], &["อืม"]),
    // --- Thai: backtracking ---
    ("นัดเจอกันตอนห้าโมง อ๊ะ ไม่สิ หกโมงเย็นนะ", &["หกโมง"], &["ห้าโมง"]),
    ("ส่งของไปที่ระยอง เอ้ย ชลบุรี", &["ชลบุรี"], &["ระยอง"]),
    // --- Thai: lists ---
    ("ต้องซื้อสามอย่าง หนึ่งนม สองไข่ สามกาแฟ", &["นม", "ไข่", "กาแฟ"], &[]),
    // --- mixed EN in Thai sentence (loanwords preserved) ---
    ("ช่วย review โค้ดให้หน่อยครับ เอ่อ ก่อนบ่ายสองนะ", &["review", "บ่ายสอง"], &["เอ่อ"]),
    // --- degenerate ---
    ("um uh hmm", &[], &["um", "uh", "hmm"]),
];

#[test]
fn golden_suite_has_at_least_30_cases() {
    assert!(GOLDEN.len() >= 30, "only {} cases", GOLDEN.len());
}

// ---------- Tier 1: deterministic rules (always run) ----------

#[test]
fn rule_tier_strips_fillers_en() {
    for (raw, _, _) in GOLDEN.iter().take(5) {
        let out = rule_based_cleanup(raw);
        for f in ["um ", "uh ", " uh", "hmm"] {
            assert!(
                !out.to_lowercase().contains(f.trim()) || raw.split_whitespace().count() < 2,
                "filler {f:?} survived in {out:?} (from {raw:?})"
            );
        }
    }
}

#[test]
fn rule_tier_strips_fillers_th() {
    let out = rule_based_cleanup("เอ่อ พรุ่งนี้ขอเลื่อนประชุมเป็นบ่ายนะครับ");
    assert!(!out.contains("เอ่อ"), "got {out:?}");
    assert!(out.contains("ประชุม"));
}

#[test]
fn rule_tier_dedups_and_capitalizes() {
    assert_eq!(rule_based_cleanup("I I want to go"), "I want to go");
    assert!(rule_based_cleanup("the answer is yes").starts_with("The"));
}

#[test]
fn rule_tier_llm_word_gate() {
    assert!(!should_use_llm("ok"));
    assert!(!should_use_llm("send it now"));
    assert!(should_use_llm("um so this needs cleanup badly"));
}

// ---------- Tier 2: real LLM (ignored unless a model is provided) ----------

#[cfg(feature = "llm-llama")]
#[test]
#[ignore = "needs LOCALFLOW_LLM_MODEL pointing at a GGUF file"]
fn llm_golden_suite() {
    let model = std::env::var("LOCALFLOW_LLM_MODEL")
        .expect("set LOCALFLOW_LLM_MODEL=/path/to/model.gguf");
    let llm = localflow_core::llm::CleanupLlm::load(std::path::Path::new(&model), 8)
        .expect("load LLM");

    let mut failures = Vec::new();
    for (i, (raw, must, must_not)) in GOLDEN.iter().enumerate() {
        // Mirror the production pipeline: deterministic filler strip first,
        // and pure-filler / tiny inputs never reach the LLM.
        let stripped = localflow_core::cleanup::pre_llm_strip(raw);
        let out = if stripped.is_empty() {
            String::new()
        } else if !should_use_llm(&stripped) {
            rule_based_cleanup(&stripped)
        } else {
            match llm.clean(&stripped, &[]) {
                Ok(o) => o,
                Err(e) => {
                    failures.push(format!("[{i}] {raw:?} -> ERROR {e:#}"));
                    continue;
                }
            }
        };
        let lo = out.to_lowercase();
        for m in *must {
            if !lo.contains(&m.to_lowercase()) {
                failures.push(format!("[{i}] {raw:?} -> {out:?} — missing {m:?}"));
            }
        }
        for m in *must_not {
            if lo.contains(&m.to_lowercase()) {
                failures.push(format!("[{i}] {raw:?} -> {out:?} — must not contain {m:?}"));
            }
        }
        println!("[{i}] {raw}\n  -> {out}");
    }
    // Small local models won't be perfect; require ≥85% of assertions clean.
    let total: usize = GOLDEN.iter().map(|(_, a, b)| a.len() + b.len()).sum();
    let pass_rate = 1.0 - failures.len() as f64 / total as f64;
    println!("\npass rate: {:.1}% ({} issues / {total} assertions)", pass_rate * 100.0, failures.len());
    for f in &failures {
        println!("FAIL {f}");
    }
    assert!(pass_rate >= 0.85, "cleanup quality below 85%: {failures:#?}");
}

// ---------- Protected words (dictionary) ----------

#[cfg(feature = "llm-llama")]
#[test]
#[ignore = "needs LOCALFLOW_LLM_MODEL pointing at a GGUF file"]
fn llm_respects_protected_words() {
    let model = std::env::var("LOCALFLOW_LLM_MODEL").unwrap();
    let llm = localflow_core::llm::CleanupLlm::load(std::path::Path::new(&model), 8).unwrap();
    let out = llm
        .clean(
            "um tell Kanchana the localflow build is ready",
            &["Kanchana".into(), "LocalFlow".into()],
        )
        .unwrap();
    assert!(out.contains("Kanchana"), "got {out:?}");
    assert!(!out.to_lowercase().contains("um "), "got {out:?}");
}
