use crate::config::AppSettings;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String, // "user" | "assistant"
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PageContext {
    pub title: String,
    pub url: String,
    pub selection: String,
    pub text: String,
    pub headings: Vec<String>,
    pub links: Vec<PageLink>,
    pub word_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageLink {
    pub text: String,
    pub href: String,
}

#[derive(Debug, Clone)]
pub enum AiStreamEvent {
    Chunk { request_id: String, text: String },
    Done { request_id: String },
    Error { request_id: String, message: String },
}

pub fn build_http_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(12))
        .build()
        .unwrap_or_else(|_| Client::new())
}

pub async fn fetch_ollama_models(base_url: &str) -> Result<Vec<String>, String> {
    let base = base_url.trim_end_matches('/');
    let url = format!("{base}/api/tags");
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    let models = body["models"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m["name"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    Ok(models)
}

fn compose_system_prompt(settings: &AppSettings, page_ctx: Option<&PageContext>) -> String {
    let mut prompt = settings.ai_system_prompt.clone();
    if let Some(ctx) = page_ctx {
        if !ctx.url.is_empty() || !ctx.text.is_empty() || !ctx.selection.is_empty() {
            prompt.push_str("\n\n--- ACTIVE WEBPAGE CONTEXT ---\n");
            if !ctx.title.is_empty() {
                prompt.push_str(&format!("Title: {}\n", ctx.title));
            }
            if !ctx.url.is_empty() {
                prompt.push_str(&format!("URL: {}\n", ctx.url));
            }
            if !ctx.selection.is_empty() {
                let sel: String = ctx.selection.chars().take(4000).collect();
                prompt.push_str(&format!("\nUser Selected Text:\n\"\"\"\n{sel}\n\"\"\"\n"));
            }
            if !ctx.headings.is_empty() {
                prompt.push_str("\nHeadings:\n");
                for h in ctx.headings.iter().take(20) {
                    prompt.push_str(&format!("- {h}\n"));
                }
            }
            if !ctx.text.is_empty() {
                let body: String = ctx.text.chars().take(16000).collect();
                prompt.push_str(&format!("\nExtracted Page Content:\n{body}\n"));
            }
            if !ctx.links.is_empty() {
                prompt.push_str("\nKey Links:\n");
                for l in ctx.links.iter().take(15) {
                    prompt.push_str(&format!("- [{}]({})\n", l.text, l.href));
                }
            }
        }
    }
    prompt
}

pub async fn run_ai_request<F>(
    client: Client,
    request_id: String,
    settings: AppSettings,
    messages: Vec<ChatMessage>,
    page_ctx: Option<PageContext>,
    mut emit: F,
) where
    F: FnMut(AiStreamEvent),
{
    let provider = settings.ai_provider.as_str();
    let result = match provider {
        "openai" => {
            if settings.openai_api_key.trim().is_empty() {
                run_local_fallback(&request_id, &messages, page_ctx.as_ref(), Some("OpenAI API key is not configured yet."), &mut emit).await
            } else {
                stream_openai_compat(
                    &client,
                    &request_id,
                    "https://api.openai.com/v1",
                    &settings.openai_api_key,
                    &settings.openai_model,
                    &compose_system_prompt(&settings, page_ctx.as_ref()),
                    &messages,
                    settings.ai_temperature,
                    &mut emit,
                )
                .await
            }
        }
        "custom" => {
            if settings.custom_base_url.trim().is_empty() {
                run_local_fallback(&request_id, &messages, page_ctx.as_ref(), Some("Custom endpoint URL is empty."), &mut emit).await
            } else {
                stream_openai_compat(
                    &client,
                    &request_id,
                    &settings.custom_base_url,
                    &settings.custom_api_key,
                    &settings.custom_model,
                    &compose_system_prompt(&settings, page_ctx.as_ref()),
                    &messages,
                    settings.ai_temperature,
                    &mut emit,
                )
                .await
            }
        }
        "anthropic" => {
            if settings.anthropic_api_key.trim().is_empty() {
                run_local_fallback(&request_id, &messages, page_ctx.as_ref(), Some("Anthropic API key is not configured yet."), &mut emit).await
            } else {
                stream_anthropic(
                    &client,
                    &request_id,
                    &settings.anthropic_api_key,
                    &settings.anthropic_model,
                    &compose_system_prompt(&settings, page_ctx.as_ref()),
                    &messages,
                    settings.ai_temperature,
                    &mut emit,
                )
                .await
            }
        }
        "gemini" => {
            if settings.gemini_api_key.trim().is_empty() {
                run_local_fallback(&request_id, &messages, page_ctx.as_ref(), Some("Gemini API key is not configured yet."), &mut emit).await
            } else {
                stream_gemini(
                    &client,
                    &request_id,
                    &settings.gemini_api_key,
                    &settings.gemini_model,
                    &compose_system_prompt(&settings, page_ctx.as_ref()),
                    &messages,
                    settings.ai_temperature,
                    &mut emit,
                )
                .await
            }
        }
        "ollama" => {
            stream_ollama(
                &client,
                &request_id,
                &settings.ollama_url,
                &settings.ollama_model,
                &compose_system_prompt(&settings, page_ctx.as_ref()),
                &messages,
                settings.ai_temperature,
                &mut emit,
            )
            .await
        }
        _ => {
            run_local_fallback(&request_id, &messages, page_ctx.as_ref(), None, &mut emit).await
        }
    };

    match result {
        Ok(()) => emit(AiStreamEvent::Done { request_id }),
        Err(message) => emit(AiStreamEvent::Error {
            request_id,
            message,
        }),
    }
}

async fn run_local_fallback<F>(
    request_id: &str,
    messages: &[ChatMessage],
    page_ctx: Option<&PageContext>,
    notice: Option<&str>,
    emit: &mut F,
) -> Result<(), String>
where
    F: FnMut(AiStreamEvent),
{
    let last_user = messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.to_lowercase())
        .unwrap_or_default();

    let mut out = String::new();
    if let Some(msg) = notice {
        out.push_str(&format!("> **Local Instant Mode** ({msg} Configure an API key or Ollama in [Settings](arthos://settings) for full LLM responses.)\n\n"));
    } else {
        out.push_str("> **Local Instant Mode** — zero-API on-device page analyzer. Connect **Ollama**, **OpenAI**, **Claude**, or **Gemini** in [Settings](arthos://settings) for conversational AI.\n\n");
    }

    if let Some(ctx) = page_ctx {
        let reading_mins = (ctx.word_count.max(1) as f32 / 220.0).ceil() as usize;
        if last_user.contains("link") {
            out.push_str(&format!("### Links extracted from **{}**\n\n", ctx.title));
            if ctx.links.is_empty() {
                out.push_str("No external links detected on this page.\n");
            } else {
                for l in ctx.links.iter().take(15) {
                    out.push_str(&format!("- [{}]({})\n", l.text, l.href));
                }
            }
        } else if !ctx.selection.is_empty() && (last_user.contains("select") || last_user.contains("explain")) {
            out.push_str("### Selected Excerpt\n\n");
            let sel: String = ctx.selection.chars().take(1200).collect();
            out.push_str(&format!("> {sel}\n\n"));
            out.push_str(&format!(
                "**Stats:** {} characters • {} words\n",
                ctx.selection.chars().count(),
                ctx.selection.split_whitespace().count()
            ));
        } else {
            out.push_str(&format!("### Page Snapshot: {}\n", if ctx.title.is_empty() { &ctx.url } else { &ctx.title }));
            out.push_str(&format!(
                "- **URL:** `{}`\n- **Length:** ~{} words (~{} min read)\n\n",
                ctx.url, ctx.word_count, reading_mins
            ));
            if !ctx.headings.is_empty() {
                out.push_str("#### Document Outline\n");
                for h in ctx.headings.iter().take(10) {
                    out.push_str(&format!("- {h}\n"));
                }
                out.push('\n');
            }
            if !ctx.text.is_empty() {
                out.push_str("#### Leading Summary Excerpt\n");
                let sentences: Vec<&str> = ctx
                    .text
                    .split_terminator(['.', '!', '?'])
                    .map(str::trim)
                    .filter(|s| s.len() > 35)
                    .take(4)
                    .collect();
                if sentences.is_empty() {
                    let snippet: String = ctx.text.chars().take(450).collect();
                    out.push_str(&snippet);
                } else {
                    for s in sentences {
                        out.push_str(&format!("- {s}.\n"));
                    }
                }
            }
        }
    } else {
        out.push_str("Open any webpage with **Page Context** enabled to analyze it locally, or connect a local **Ollama** / cloud model in [arthos://settings](arthos://settings).");
    }

    emit(AiStreamEvent::Chunk {
        request_id: request_id.to_string(),
        text: out,
    });
    Ok(())
}

async fn stream_openai_compat<F>(
    client: &Client,
    request_id: &str,
    base_url: &str,
    api_key: &str,
    model: &str,
    system_prompt: &str,
    messages: &[ChatMessage],
    temperature: f32,
    emit: &mut F,
) -> Result<(), String>
where
    F: FnMut(AiStreamEvent),
{
    let base = base_url.trim_end_matches('/');
    let url = if base.ends_with("/chat/completions") {
        base.to_string()
    } else {
        format!("{base}/chat/completions")
    };

    let mut api_messages = Vec::with_capacity(messages.len() + 1);
    api_messages.push(json!({ "role": "system", "content": system_prompt }));
    for m in messages {
        api_messages.push(json!({ "role": m.role, "content": m.content }));
    }

    let mut req = client.post(&url).json(&json!({
        "model": model,
        "messages": api_messages,
        "temperature": temperature,
        "stream": true
    }));
    if !api_key.trim().is_empty() {
        req = req.bearer_auth(api_key.trim());
    }

    let resp = req.send().await.map_err(|e| format!("Network error: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("API error ({status}): {text}"));
    }

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    while let Some(item) = stream.next().await {
        let bytes = item.map_err(|e| format!("Stream read error: {e}"))?;
        buffer.push_str(&String::from_utf8_lossy(&bytes));
        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer.drain(..=pos);
            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    return Ok(());
                }
                if let Ok(val) = serde_json::from_str::<Value>(data) {
                    if let Some(delta) = val["choices"][0]["delta"]["content"].as_str() {
                        if !delta.is_empty() {
                            emit(AiStreamEvent::Chunk {
                                request_id: request_id.to_string(),
                                text: delta.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

async fn stream_anthropic<F>(
    client: &Client,
    request_id: &str,
    api_key: &str,
    model: &str,
    system_prompt: &str,
    messages: &[ChatMessage],
    temperature: f32,
    emit: &mut F,
) -> Result<(), String>
where
    F: FnMut(AiStreamEvent),
{
    let api_messages: Vec<Value> = messages
        .iter()
        .map(|m| json!({ "role": m.role, "content": m.content }))
        .collect();

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key.trim())
        .header("anthropic-version", "2023-06-01")
        .json(&json!({
            "model": model,
            "max_tokens": 4096,
            "system": system_prompt,
            "messages": api_messages,
            "temperature": temperature,
            "stream": true
        }))
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Anthropic API error ({status}): {text}"));
    }

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    while let Some(item) = stream.next().await {
        let bytes = item.map_err(|e| format!("Stream read error: {e}"))?;
        buffer.push_str(&String::from_utf8_lossy(&bytes));
        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer.drain(..=pos);
            if let Some(data) = line.strip_prefix("data: ") {
                if let Ok(val) = serde_json::from_str::<Value>(data) {
                    if val["type"] == "content_block_delta" {
                        if let Some(delta) = val["delta"]["text"].as_str() {
                            if !delta.is_empty() {
                                emit(AiStreamEvent::Chunk {
                                    request_id: request_id.to_string(),
                                    text: delta.to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

async fn stream_gemini<F>(
    client: &Client,
    request_id: &str,
    api_key: &str,
    model: &str,
    system_prompt: &str,
    messages: &[ChatMessage],
    temperature: f32,
    emit: &mut F,
) -> Result<(), String>
where
    F: FnMut(AiStreamEvent),
{
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
        model.trim(),
        api_key.trim()
    );

    let contents: Vec<Value> = messages
        .iter()
        .map(|m| {
            let role = if m.role == "assistant" { "model" } else { "user" };
            json!({
                "role": role,
                "parts": [{ "text": m.content }]
            })
        })
        .collect();

    let resp = client
        .post(&url)
        .json(&json!({
            "systemInstruction": { "parts": [{ "text": system_prompt }] },
            "contents": contents,
            "generationConfig": { "temperature": temperature }
        }))
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Gemini API error ({status}): {text}"));
    }

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    while let Some(item) = stream.next().await {
        let bytes = item.map_err(|e| format!("Stream read error: {e}"))?;
        buffer.push_str(&String::from_utf8_lossy(&bytes));
        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer.drain(..=pos);
            if let Some(data) = line.strip_prefix("data: ") {
                if let Ok(val) = serde_json::from_str::<Value>(data) {
                    if let Some(parts) = val["candidates"][0]["content"]["parts"].as_array() {
                        for p in parts {
                            if let Some(text) = p["text"].as_str() {
                                if !text.is_empty() {
                                    emit(AiStreamEvent::Chunk {
                                        request_id: request_id.to_string(),
                                        text: text.to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

async fn stream_ollama<F>(
    client: &Client,
    request_id: &str,
    base_url: &str,
    model: &str,
    system_prompt: &str,
    messages: &[ChatMessage],
    temperature: f32,
    emit: &mut F,
) -> Result<(), String>
where
    F: FnMut(AiStreamEvent),
{
    let base = base_url.trim_end_matches('/');
    let url = format!("{base}/api/chat");

    let mut api_messages = Vec::with_capacity(messages.len() + 1);
    api_messages.push(json!({ "role": "system", "content": system_prompt }));
    for m in messages {
        api_messages.push(json!({ "role": m.role, "content": m.content }));
    }

    let resp = client
        .post(&url)
        .json(&json!({
            "model": model,
            "messages": api_messages,
            "options": { "temperature": temperature },
            "stream": true
        }))
        .send()
        .await
        .map_err(|e| format!("Could not reach Ollama at {base}: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Ollama error ({status}): {text}"));
    }

    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    while let Some(item) = stream.next().await {
        let bytes = item.map_err(|e| format!("Stream read error: {e}"))?;
        buffer.push_str(&String::from_utf8_lossy(&bytes));
        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer.drain(..=pos);
            if line.is_empty() {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<Value>(&line) {
                if let Some(delta) = val["message"]["content"].as_str() {
                    if !delta.is_empty() {
                        emit(AiStreamEvent::Chunk {
                            request_id: request_id.to_string(),
                            text: delta.to_string(),
                        });
                    }
                }
                if val["done"].as_bool() == Some(true) {
                    return Ok(());
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_instant_mode_summarizes_page_context() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let client = build_http_client();
            let settings = AppSettings::default();
            let messages = vec![ChatMessage {
                role: "user".into(),
                content: "Summarize this page".into(),
            }];
            let ctx = PageContext {
                title: "Arthos Docs".into(),
                url: "https://example.com/docs".into(),
                selection: String::new(),
                text: "Arthos is a fast native Rust web browser with integrated AI capabilities. It uses OS webviews for minimal memory usage.".into(),
                headings: vec!["Overview".into(), "Architecture".into()],
                links: vec![PageLink {
                    text: "GitHub".into(),
                    href: "https://github.com".into(),
                }],
                word_count: 20,
            };
            let mut events = Vec::new();
            run_ai_request(client, "req-1".into(), settings, messages, Some(ctx), |ev| {
                events.push(ev);
            })
            .await;

            assert!(events.iter().any(|e| matches!(e, AiStreamEvent::Chunk { text, .. } if text.contains("Arthos Docs"))));
            assert!(matches!(events.last(), Some(AiStreamEvent::Done { .. })));
        });
    }
}
