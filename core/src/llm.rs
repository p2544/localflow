//! Transcript cleanup LLM via llama.cpp (llama-cpp-2 bindings).
//!
//! Runs a small instruct model (default Qwen2.5-3B-Instruct Q4) with
//! temperature 0 and a strict edit-only prompt. The context is kept warm
//! between dictations; `Pipeline` handles idle unloading.

use crate::cleanup::{build_user_prompt, CLEANUP_FEW_SHOT, CLEANUP_SYSTEM_PROMPT};
use anyhow::{anyhow, Context, Result};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel, Special};
use llama_cpp_2::sampling::LlamaSampler;
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::OnceLock;

const N_CTX: u32 = 4096;
const MAX_OUTPUT_TOKENS: usize = 1024;

fn backend() -> Result<&'static LlamaBackend> {
    static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();
    if BACKEND.get().is_none() {
        let b = LlamaBackend::init().context("init llama backend")?;
        let _ = BACKEND.set(b);
    }
    BACKEND.get().ok_or_else(|| anyhow!("llama backend missing"))
}

pub struct CleanupLlm {
    model: LlamaModel,
    threads: i32,
}

impl CleanupLlm {
    pub fn load(model_path: &Path, threads: usize) -> Result<Self> {
        let backend = backend()?;
        // n_gpu_layers = max: offload everything the build supports (Metal on
        // macOS, Vulkan/CUDA on Windows when compiled in); silently CPU otherwise.
        let params = LlamaModelParams::default().with_n_gpu_layers(1_000_000);
        let model = LlamaModel::load_from_file(backend, model_path, &params)
            .context("failed to load LLM gguf")?;
        Ok(Self {
            model,
            threads: threads as i32,
        })
    }

    /// One cleanup call: transcript in, edited text out. Deterministic
    /// (greedy sampling), stops at EOS or MAX_OUTPUT_TOKENS.
    pub fn clean(&self, transcript: &str, protected_words: &[String]) -> Result<String> {
        let backend = backend()?;
        let mut messages =
            vec![LlamaChatMessage::new("system".into(), CLEANUP_SYSTEM_PROMPT.into())?];
        for (raw, cleaned) in CLEANUP_FEW_SHOT {
            messages.push(LlamaChatMessage::new(
                "user".into(),
                build_user_prompt(raw, &[]),
            )?);
            messages.push(LlamaChatMessage::new("assistant".into(), (*cleaned).into())?);
        }
        messages.push(LlamaChatMessage::new(
            "user".into(),
            build_user_prompt(transcript, protected_words),
        )?);

        let template = self.model.chat_template(None).ok();
        let prompt = match template {
            Some(tpl) => self
                .model
                .apply_chat_template(&tpl, &messages, true)
                .context("apply chat template")?,
            // Fallback: ChatML, which Qwen/Llama-3-style GGUFs understand.
            None => {
                let mut s = format!(
                    "<|im_start|>system\n{CLEANUP_SYSTEM_PROMPT}<|im_end|>\n"
                );
                for (raw, cleaned) in CLEANUP_FEW_SHOT {
                    s.push_str(&format!(
                        "<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n{}<|im_end|>\n",
                        build_user_prompt(raw, &[]),
                        cleaned
                    ));
                }
                s.push_str(&format!(
                    "<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
                    build_user_prompt(transcript, protected_words)
                ));
                s
            }
        };

        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(N_CTX))
            .with_n_threads(self.threads)
            .with_n_threads_batch(self.threads);
        let mut ctx = self
            .model
            .new_context(backend, ctx_params)
            .context("create llama context")?;

        let tokens = self
            .model
            .str_to_token(&prompt, AddBos::Always)
            .context("tokenize prompt")?;
        if tokens.len() as u32 >= N_CTX - 64 {
            return Err(anyhow!("transcript too long for cleanup context"));
        }

        let mut batch = LlamaBatch::new(tokens.len().max(512), 1);
        let last = tokens.len() - 1;
        for (i, tok) in tokens.iter().enumerate() {
            batch.add(*tok, i as i32, &[0], i == last)?;
        }
        ctx.decode(&mut batch).context("prompt decode")?;

        let mut sampler = LlamaSampler::greedy();
        let mut out = String::new();
        let mut n_cur = tokens.len() as i32;
        for _ in 0..MAX_OUTPUT_TOKENS {
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            if self.model.is_eog_token(token) {
                break;
            }
            #[allow(deprecated)] // token_to_piece needs a persistent decoder; fine here
            let piece = self
                .model
                .token_to_str(token, Special::Tokenize)
                .unwrap_or_default();
            out.push_str(&piece);
            batch.clear();
            batch.add(token, n_cur, &[0], true)?;
            n_cur += 1;
            ctx.decode(&mut batch).context("gen decode")?;
        }
        Ok(crate::cleanup::sanitize_llm_output(&out))
    }
}
