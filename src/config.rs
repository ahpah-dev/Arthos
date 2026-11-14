use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub theme: String,        // "dark" | "light"
    pub accent: String,       // "indigo" | "emerald" | "amber" | "rose"
    pub search_engine: String, // "duckduckgo" | "google" | "brave" | "kagi" | "perplexity"
    pub homepage: String,
    pub restore_session: bool,
    pub selection_pill: bool,
    pub include_page_context: bool,
    pub ai_provider: String, // "local" | "openai" | "anthropic" | "gemini" | "ollama" | "custom"
    pub openai_api_key: String,
    pub openai_model: String,
    pub anthropic_api_key: String,
    pub anthropic_model: String,
    pub gemini_api_key: String,
    pub gemini_model: String,
    pub ollama_url: String,
    pub ollama_model: String,
    pub custom_base_url: String,
    pub custom_api_key: String,
    pub custom_model: String,
    pub ai_system_prompt: String,
    pub ai_temperature: f32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            accent: "indigo".to_string(),
            search_engine: "duckduckgo".to_string(),
            homepage: "arthos://newtab".to_string(),
            restore_session: true,
            selection_pill: true,
            include_page_context: true,
            ai_provider: "local".to_string(),
            openai_api_key: String::new(),
            openai_model: "gpt-4o-mini".to_string(),
            anthropic_api_key: String::new(),
            anthropic_model: "claude-sonnet-4-20250514".to_string(),
            gemini_api_key: String::new(),
            gemini_model: "gemini-2.5-flash".to_string(),
            ollama_url: "http://localhost:11434".to_string(),
            ollama_model: "llama3.2".to_string(),
            custom_base_url: "https://api.groq.com/openai/v1".to_string(),
            custom_api_key: String::new(),
            custom_model: "llama-3.3-70b-versatile".to_string(),
            ai_system_prompt: "You are Arthos AI, a concise, accurate, privacy-respecting browsing assistant built into the Arthos web browser. Format answers clearly with Markdown when helpful.".to_string(),
            ai_temperature: 0.4,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub title: String,
    pub url: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryItem {
    pub id: String,
    pub title: String,
    pub url: String,
    pub visited_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeedDialTile {
    pub title: String,
    pub url: String,
    pub icon: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadItem {
    pub id: String,
    pub filename: String,
    pub url: String,
    pub path: String,
    pub status: String, // "downloading" | "completed" | "failed"
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedState {
    pub settings: AppSettings,
    pub bookmarks: Vec<Bookmark>,
    pub history: Vec<HistoryItem>,
    pub speed_dial: Vec<SpeedDialTile>,
    pub downloads: Vec<DownloadItem>,
    pub open_tabs: Vec<String>,
    pub active_tab_index: usize,
    pub sidebar_open: bool,
    pub sidebar_width: u32,
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            settings: AppSettings::default(),
            bookmarks: vec![
                Bookmark {
                    id: "bm-1".into(),
                    title: "GitHub".into(),
                    url: "https://github.com".into(),
                    created_at: Utc::now(),
                },
                Bookmark {
                    id: "bm-2".into(),
                    title: "Wikipedia".into(),
                    url: "https://www.wikipedia.org".into(),
                    created_at: Utc::now(),
                },
                Bookmark {
                    id: "bm-3".into(),
                    title: "Hacker News".into(),
                    url: "https://news.ycombinator.com".into(),
                    created_at: Utc::now(),
                },
            ],
            history: Vec::new(),
            speed_dial: vec![
                SpeedDialTile {
                    title: "GitHub".into(),
                    url: "https://github.com".into(),
                    icon: "GH".into(),
                },
                SpeedDialTile {
                    title: "Wikipedia".into(),
                    url: "https://www.wikipedia.org".into(),
                    icon: "W".into(),
                },
                SpeedDialTile {
                    title: "Hacker News".into(),
                    url: "https://news.ycombinator.com".into(),
                    icon: "HN".into(),
                },
                SpeedDialTile {
                    title: "ArXiv".into(),
                    url: "https://arxiv.org".into(),
                    icon: "AX".into(),
                },
                SpeedDialTile {
                    title: "Rust Docs".into(),
                    url: "https://doc.rust-lang.org".into(),
                    icon: "RS".into(),
                },
                SpeedDialTile {
                    title: "DuckDuckGo".into(),
                    url: "https://duckduckgo.com".into(),
                    icon: "DD".into(),
                },
            ],
            downloads: Vec::new(),
            open_tabs: Vec::new(),
            active_tab_index: 0,
            sidebar_open: false,
            sidebar_width: 390,
        }
    }
}

pub fn app_data_dir() -> PathBuf {
    let base = dirs::data_local_dir()
        .or_else(dirs::config_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("Arthos");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn webview_profile_dir() -> PathBuf {
    let dir = app_data_dir().join("profile");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn state_file_path() -> PathBuf {
    app_data_dir().join("state.json")
}

pub fn load_state() -> PersistedState {
    let path = state_file_path();
    match fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(raw.trim_start_matches('\u{feff}')).unwrap_or_default(),
        Err(_) => PersistedState::default(),
    }
}

pub fn save_state(state: &PersistedState) {
    let path = state_file_path();
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = fs::write(path, json);
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum OmniboxAction {
    Navigate(String),
    AskAi(String),
}

pub fn to_internal_arthos_url(display_or_internal: &str) -> String {
    let trimmed = display_or_internal.trim();
    if let Some(rest) = trimmed.strip_prefix("arthos://") {
        let rest = rest.trim_start_matches('/');
        if rest.starts_with("localhost/") || rest == "localhost" {
            return format!("arthos://{rest}");
        }
        let route = if rest.is_empty() { "newtab" } else { rest };
        return format!("arthos://localhost/{route}");
    }
    trimmed.to_string()
}

pub fn to_display_url(raw_url: &str) -> String {
    if let Some(rest) = raw_url.strip_prefix("http://arthos.localhost/") {
        return format!("arthos://{rest}");
    }
    if raw_url == "http://arthos.localhost" || raw_url == "http://arthos.localhost/" {
        return "arthos://newtab".to_string();
    }
    if let Some(rest) = raw_url.strip_prefix("https://arthos.localhost/") {
        return format!("arthos://{rest}");
    }
    if let Some(rest) = raw_url.strip_prefix("arthos://localhost/") {
        return format!("arthos://{rest}");
    }
    if raw_url == "arthos://localhost" || raw_url == "arthos://localhost/" {
        return "arthos://newtab".to_string();
    }
    raw_url.to_string()
}

pub fn search_url_for(engine: &str, query: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    match engine {
        "google" => format!("https://www.google.com/search?q={encoded}"),
        "brave" => format!("https://search.brave.com/search?q={encoded}"),
        "kagi" => format!("https://kagi.com/search?q={encoded}"),
        "perplexity" => format!("https://www.perplexity.ai/search?q={encoded}"),
        _ => format!("https://duckduckgo.com/?q={encoded}"),
    }
}

pub fn resolve_omnibox_input(input: &str, search_engine: &str) -> OmniboxAction {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return OmniboxAction::Navigate(to_internal_arthos_url("arthos://newtab"));
    }

    if let Some(q) = trimmed.strip_prefix("? ") {
        return OmniboxAction::AskAi(q.trim().to_string());
    }
    if let Some(q) = trimmed.strip_prefix("/ai ") {
        return OmniboxAction::AskAi(q.trim().to_string());
    }
    if let Some(q) = trimmed.strip_prefix("/ask ") {
        return OmniboxAction::AskAi(q.trim().to_string());
    }

    if trimmed.starts_with("arthos://") {
        return OmniboxAction::Navigate(to_internal_arthos_url(trimmed));
    }

    if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("file://")
    {
        return OmniboxAction::Navigate(trimmed.to_string());
    }

    // Check if it looks like localhost:PORT or domain.tld(/path) without spaces
    if !trimmed.contains(' ') {
        let is_localhost = trimmed == "localhost"
            || trimmed.starts_with("localhost:")
            || trimmed.starts_with("localhost/")
            || trimmed.starts_with("127.0.0.1");
        if is_localhost {
            return OmniboxAction::Navigate(format!("http://{trimmed}"));
        }

        if let Some((host_part, _)) = trimmed.split_once('/').or(Some((trimmed, ""))) {
            if host_part.contains('.') {
                let candidate = format!("https://{trimmed}");
                if Url::parse(&candidate).is_ok() {
                    return OmniboxAction::Navigate(candidate);
                }
            }
        }
    }

    OmniboxAction::Navigate(search_url_for(search_engine, trimmed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_urls_searches_and_ai_queries() {
        assert_eq!(
            resolve_omnibox_input("github.com/rust-lang", "duckduckgo"),
            OmniboxAction::Navigate("https://github.com/rust-lang".into())
        );
        assert_eq!(
            resolve_omnibox_input("localhost:3000/app", "duckduckgo"),
            OmniboxAction::Navigate("http://localhost:3000/app".into())
        );
        assert_eq!(
            resolve_omnibox_input("arthos://settings", "duckduckgo"),
            OmniboxAction::Navigate("arthos://localhost/settings".into())
        );
        assert_eq!(
            resolve_omnibox_input("? summarize this", "duckduckgo"),
            OmniboxAction::AskAi("summarize this".into())
        );
        assert_eq!(
            resolve_omnibox_input("rust async streams", "google"),
            OmniboxAction::Navigate("https://www.google.com/search?q=rust+async+streams".into())
        );
    }

    #[test]
    fn converts_internal_urls_for_display() {
        assert_eq!(
            to_display_url("http://arthos.localhost/settings"),
            "arthos://settings"
        );
        assert_eq!(
            to_display_url("arthos://localhost/newtab"),
            "arthos://newtab"
        );
    }
}
