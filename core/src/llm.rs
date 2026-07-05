//! Transcript cleanup LLM via llama.cpp (llama-cpp-2 bindings).
//!
//! Runs a small instruct model (default Qwen2.5-3B-Instruct Q4) with greedy
//! sampling and a strict edit-only prompt.
//!
//! Latency design: `LlamaContext` borrows `LlamaModel`, so both live on a
//! dedicated worker thread. The context is persistent and we reuse the KV
//! cache across calls — each request tokenizes the full chat prompt, finds
//! the longest common token prefix with what is already in cache (in steady
//! state: the whole system + few-shot block, ~700 tokens), evicts only the
//! divergent tail, and decodes just the new user turn. That turns multi-
//! second CPU prompt processing into tens of tokens per call.

use crate::cleanup::{build_user_prompt, CLEANUP_FEW_SHOT, CLEANUP_SYSTEM_PROMPT};
use anyhow::{anyhow, Context as _, Result};
use crossbeam_channel::{bounded, Sender};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaChatTemplate, LlamaModel, Special};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
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

type Job = (String, Vec<String>, Sender<Result<String>>);

/// Handle to the LLM worker thread. Dropping it shuts the thread down.
pub struct CleanupLlm {
    tx: Sender<Job>,
}

impl CleanupLlm {
    pub fn load(model_path: &Path, threads: usize) -> Result<Self> {
        let (tx, rx) = bounded::<Job>(8);
        let (ready_tx, ready_rx) = bounded::<Result<()>>(1);
        let path = model_path.to_path_buf();
        std::thread::Builder::new()
            .name("localflow-llm".into())
            .spawn(move || {
                let mut worker = match LlmWorker::new(&path, threads) {
                    Ok(w) => {
                        let _ = ready_tx.send(Ok(()));
                        w
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                while let Ok((transcript, protected, reply)) = rx.recv() {
                    let _ = reply.send(worker.clean(&transcript, &protected));
                }
            })
            .context("spawn llm thread")?;
        ready_rx
            .recv()
            .map_err(|_| anyhow!("llm thread died during load"))??;
        Ok(Self { tx })
    }

    /// One cleanup call: transcript in, edited text out.
    pub fn clean(&self, transcript: &str, protected_words: &[String]) -> Result<String> {
        let (rtx, rrx) = bounded(1);
        self.tx
            .send((transcript.to_string(), protected_words.to_vec(), rtx))
            .map_err(|_| anyhow!("llm thread gone"))?;
        rrx.recv().map_err(|_| anyhow!("llm thread gone"))?
    }
}

struct LlmWorker {
    model: &'static LlamaModel,
    ctx: LlamaContext<'static>,
    template: Option<LlamaChatTemplate>,
    /// Tokens currently materialized in the KV cache (prompt + last output).
    cached: Vec<LlamaToken>,
}

impl LlmWorker {
    fn new(path: &PathBuf, threads: usize) -> Result<Self> {
        let backend = backend()?;
        // Offload everything the build supports (Metal/Vulkan/CUDA); CPU otherwise.
        let params = LlamaModelParams::default().with_n_gpu_layers(1_000_000);
        let model = LlamaModel::load_from_file(backend, path, &params)
            .context("failed to load LLM gguf")?;
        // The model must outlive the context stored beside it; the worker
        // thread owns both for the process lifetime, so leaking is correct
        // (one model per thread, freed at process exit).
        let model: &'static LlamaModel = Box::leak(Box::new(model));

        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(N_CTX))
            .with_n_threads(threads as i32)
            .with_n_threads_batch(threads as i32);
        let ctx = model
            .new_context(backend, ctx_params)
            .context("create llama context")?;
        let template = model.chat_template(None).ok();
        let mut worker = Self { model, ctx, template, cached: Vec::new() };
        // Prewarm: decode the static system + few-shot prefix now so the
        // first real dictation only pays for its own user turn.
        if let Err(e) = worker.prewarm() {
            tracing::warn!("LLM prefix prewarm failed (non-fatal): {e:#}");
        }
        Ok(worker)
    }

    /// Decodes the prompt for an empty transcript, which shares its token
    /// prefix (system + few-shots + "TRANSCRIPT: ") with every real prompt.
    fn prewarm(&mut self) -> Result<()> {
        let prompt = self.build_prompt("", &[])?;
        let tokens = self.model.str_to_token(&prompt, AddBos::Always)?;
        let mut batch = LlamaBatch::new(tokens.len(), 1);
        let last = tokens.len() - 1;
        for (i, tok) in tokens.iter().enumerate() {
            batch.add(*tok, i as i32, &[0], i == last)?;
        }
        self.ctx.decode(&mut batch).context("prewarm decode")?;
        self.cached = tokens;
        Ok(())
    }

    fn build_prompt(&self, transcript: &str, protected: &[String]) -> Result<String> {
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
            build_user_prompt(transcript, protected),
        )?);

        match &self.template {
            Some(tpl) => self
                .model
                .apply_chat_template(tpl, &messages, true)
                .context("apply chat template"),
            // Fallback: ChatML, which Qwen/Llama-3-style GGUFs understand.
            None => {
                let mut s =
                    format!("<|im_start|>system\n{CLEANUP_SYSTEM_PROMPT}<|im_end|>\n");
                for (raw, cleaned) in CLEANUP_FEW_SHOT {
                    s.push_str(&format!(
                        "<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n{}<|im_end|>\n",
                        build_user_prompt(raw, &[]),
                        cleaned
                    ));
                }
                s.push_str(&format!(
                    "<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
                    build_user_prompt(transcript, protected)
                ));
                Ok(s)
            }
        }
    }

    fn clean(&mut self, transcript: &str, protected: &[String]) -> Result<String> {
        let prompt = self.build_prompt(transcript, protected)?;
        let tokens = self
            .model
            .str_to_token(&prompt, AddBos::Always)
            .context("tokenize prompt")?;
        if tokens.len() as u32 >= N_CTX - 64 {
            return Err(anyhow!("transcript too long for cleanup context"));
        }

        // KV reuse: keep the longest common prefix, evict the rest.
        let common = self
            .cached
            .iter()
            .zip(&tokens)
            .take_while(|(a, b)| a == b)
            .count()
            // Always re-decode at least the final prompt token so decode()
            // has fresh logits to sample from.
            .min(tokens.len() - 1);
        self.ctx
            .clear_kv_cache_seq(Some(0), Some(common as u32), None)
            .map_err(|e| anyhow!("kv cache trim: {e}"))?;

        let mut batch = LlamaBatch::new((tokens.len() - common).max(64), 1);
        let last = tokens.len() - 1;
        for (i, tok) in tokens.iter().enumerate().skip(common) {
            batch.add(*tok, i as i32, &[0], i == last)?;
        }
        self.ctx.decode(&mut batch).context("prompt decode")?;
        self.cached = tokens.clone();

        let mut sampler = LlamaSampler::greedy();
        let mut out = String::new();
        let mut n_cur = tokens.len() as i32;
        for _ in 0..MAX_OUTPUT_TOKENS {
            let token = sampler.sample(&self.ctx, batch.n_tokens() - 1);
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
            self.ctx.decode(&mut batch).context("gen decode")?;
            self.cached.push(token);
        }
        Ok(crate::cleanup::sanitize_llm_output(&out))
    }
}
