use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PulseTelemetry {
    pub ttft_ms: u64,
    pub total_ms: u64,
    pub tokens_est: usize,
    pub tok_per_sec: f64,
    pub compression_pct: u8,
    pub cache_hit: bool,
    pub engine_tag: String,
}

#[derive(Debug, Clone)]
pub struct CompressedContext {
    pub text: String,
    pub original_chars: usize,
    pub compressed_chars: usize,
    pub reduction_pct: u8,
}

/// Fast structural context compressor for webpage text.
/// Deduplicates repetitive boilerplate lines, collapses whitespace, and keeps high-density
/// content within `max_chars` so LLMs achieve faster Time-To-First-Token (TTFT).
pub fn compress_context(raw: &str, max_chars: usize) -> CompressedContext {
    let original_chars = raw.chars().count();
    if original_chars == 0 {
        return CompressedContext {
            text: String::new(),
            original_chars: 0,
            compressed_chars: 0,
            reduction_pct: 0,
        };
    }

    let mut seen = HashSet::new();
    let mut kept = Vec::new();
    let mut used_chars = 0usize;

    for line in raw.lines() {
        let trimmed = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if trimmed.len() < 4 {
            continue;
        }
        // Skip duplicate navigation/cookie/footer fragments
        let norm = trimmed.to_lowercase();
        if !seen.insert(norm.clone()) {
            continue;
        }
        if norm.starts_with("cookie") && trimmed.len() < 90 {
            continue;
        }
        let line_len = trimmed.chars().count() + 1;
        if used_chars + line_len > max_chars {
            break;
        }
        used_chars += line_len;
        kept.push(trimmed);
    }

    let text = kept.join("\n");
    let compressed_chars = text.chars().count();
    let reduction_pct = if original_chars > compressed_chars {
        (((original_chars - compressed_chars) as f64 / original_chars as f64) * 100.0)
            .round()
            .clamp(0.0, 99.0) as u8
    } else {
        0
    };

    CompressedContext {
        text,
        original_chars,
        compressed_chars,
        reduction_pct,
    }
}

/// Bounded in-memory LRU cache for repeat AI page summaries/queries.
pub struct PulseResponseCache {
    capacity: usize,
    ttl: Duration,
    entries: HashMap<u64, (Instant, String)>,
    order: VecDeque<u64>,
}

impl PulseResponseCache {
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            capacity,
            ttl,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn make_key(provider: &str, model: &str, prompt: &str, context_sig: &str) -> u64 {
        let mut h = DefaultHasher::new();
        provider.hash(&mut h);
        model.hash(&mut h);
        prompt.trim().hash(&mut h);
        context_sig.hash(&mut h);
        h.finish()
    }

    pub fn get(&mut self, key: u64) -> Option<String> {
        if let Some((ts, val)) = self.entries.get(&key) {
            if ts.elapsed() <= self.ttl {
                return Some(val.clone());
            }
            self.entries.remove(&key);
        }
        None
    }

    pub fn insert(&mut self, key: u64, response: String) {
        if response.trim().is_empty() {
            return;
        }
        if self.entries.len() >= self.capacity {
            if let Some(old) = self.order.pop_front() {
                self.entries.remove(&old);
            }
        }
        self.entries.insert(key, (Instant::now(), response));
        self.order.push_back(key);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }
}

/// Builds a pre-tuned persistent HTTP/2 client with TCP_NODELAY and pooled keep-alive.
pub fn build_pulse_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(10))
        .tcp_nodelay(true)
        .pool_idle_timeout(Duration::from_secs(90))
        .pool_max_idle_per_host(8)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// Pre-warms DNS/TCP/TLS connection to an AI endpoint in the background.
pub async fn warmup_endpoint(client: &reqwest::Client, url: &str) {
    let _ = client
        .head(url)
        .timeout(Duration::from_secs(3))
        .send()
        .await;
}
