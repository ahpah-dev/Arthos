use crate::ai::{ChatMessage, PageContext};
use crate::config::{
    AppSettings, Bookmark, DownloadItem, HistoryItem, PersistedState, SpeedDialTile, to_display_url,
};
use serde::{Deserialize, Serialize};
use wry::WebView;

pub const CHROME_BASE_HEIGHT: u32 = 86;
pub const FIND_BAR_HEIGHT: u32 = 42;
pub const MIN_SIDEBAR_WIDTH: u32 = 300;
pub const MAX_SIDEBAR_WIDTH: u32 = 620;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReaderArticle {
    pub title: String,
    pub url: String,
    pub byline: String,
    pub site_name: String,
    pub html: String,
    pub word_count: usize,
}

pub struct Tab {
    pub id: u64,
    pub title: String,
    pub url: String,
    pub loading: bool,
    pub incognito: bool,
    pub zoom: f64,
    pub reader_origin_url: Option<String>,
    pub webview: WebView,
}

#[derive(Debug, Clone, Serialize)]
pub struct TabDto {
    pub id: u64,
    pub title: String,
    pub url: String,
    pub display_url: String,
    pub loading: bool,
    pub incognito: bool,
    pub zoom: f64,
    pub is_bookmarked: bool,
    pub is_reader: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChromeSyncPayload {
    pub tabs: Vec<TabDto>,
    pub active_tab_id: u64,
    pub sidebar_open: bool,
    pub sidebar_width: u32,
    pub find_open: bool,
    pub overlay_open: bool,
    pub theme: String,
    pub accent: String,
    pub ai_provider: String,
    pub active_model: String,
    pub selection_pill: bool,
    pub include_page_context: bool,
    pub is_maximized: bool,
    pub downloads: Vec<DownloadItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InternalPagePayload {
    pub settings: AppSettings,
    pub bookmarks: Vec<Bookmark>,
    pub history: Vec<HistoryItem>,
    pub speed_dial: Vec<SpeedDialTile>,
    pub downloads: Vec<DownloadItem>,
    pub reader_article: Option<ReaderArticle>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum FrontendCommand {
    WindowMinimize,
    WindowMaximize,
    WindowClose,
    WindowDrag,
    NewTab {
        url: Option<String>,
        incognito: Option<bool>,
    },
    SwitchTab {
        id: u64,
    },
    CloseTab {
        id: u64,
    },
    NextTab,
    PrevTab,
    Navigate {
        input: String,
    },
    NavBack,
    NavForward,
    NavReload,
    NavHome,
    ToggleBookmark,
    OpenDevtools,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ToggleFindBar {
        open: Option<bool>,
    },
    FindInPage {
        query: String,
        backward: Option<bool>,
    },
    ToggleReaderMode,
    ToggleSidebar {
        open: Option<bool>,
    },
    SetSidebarWidth {
        width: u32,
    },
    ToggleOverlay {
        open: Option<bool>,
        mode: Option<String>,
        query: Option<String>,
    },
    FocusOmnibox,
    AiAsk {
        request_id: String,
        messages: Vec<ChatMessage>,
        include_page: bool,
    },
    AiStop,
    AiSetProvider {
        provider: String,
        model: Option<String>,
    },
    AskSelection {
        text: String,
        action: String,
    },
    GrabSelection,
    GetState,
    SaveSettings {
        settings: AppSettings,
    },
    AddSpeedDial {
        title: String,
        url: String,
    },
    RemoveSpeedDial {
        url: String,
    },
    DeleteBookmark {
        id: String,
    },
    ClearHistory,
    ClearBookmarks,
    ClearDownloads,
    DetectOllama,
    OpenExternalTab {
        url: String,
    },
    ShowToast {
        message: String,
    },
}

#[derive(Debug, Clone)]
pub enum AppEvent {
    IpcMessage(String),
    TabTitleChanged {
        tab_id: u64,
        title: String,
    },
    TabPageLoad {
        tab_id: u64,
        url: String,
        finished: bool,
    },
    OpenNewTabUrl {
        url: String,
        incognito: bool,
    },
    DownloadStarted {
        id: String,
        url: String,
        filename: String,
        path: String,
    },
    DownloadCompleted {
        url: String,
        path: Option<String>,
        success: bool,
    },
    AiContextReady {
        request_id: String,
        messages: Vec<ChatMessage>,
        context: Option<PageContext>,
    },
    AiStream(crate::ai::AiStreamEvent),
    OllamaModelsDetected(Result<Vec<String>, String>),
    ReaderExtracted(Result<ReaderArticle, String>),
    SelectionGrabbed(String),
}

pub fn active_model_label(s: &AppSettings) -> String {
    match s.ai_provider.as_str() {
        "openai" => s.openai_model.clone(),
        "anthropic" => s.anthropic_model.clone(),
        "gemini" => s.gemini_model.clone(),
        "ollama" => s.ollama_model.clone(),
        "custom" => s.custom_model.clone(),
        _ => "Local Instant".to_string(),
    }
}

pub fn build_chrome_payload(
    tabs: &[Tab],
    active_tab_id: u64,
    persisted: &PersistedState,
    find_open: bool,
    overlay_open: bool,
    is_maximized: bool,
) -> ChromeSyncPayload {
    let tab_dtos = tabs
        .iter()
        .map(|t| {
            let display_url = to_display_url(&t.url);
            let is_bookmarked = persisted
                .bookmarks
                .iter()
                .any(|b| b.url == t.url || b.url == display_url);
            TabDto {
                id: t.id,
                title: if t.title.is_empty() {
                    if t.incognito {
                        "Private Tab".to_string()
                    } else {
                        "New Tab".to_string()
                    }
                } else {
                    t.title.clone()
                },
                url: t.url.clone(),
                display_url,
                loading: t.loading,
                incognito: t.incognito,
                zoom: t.zoom,
                is_bookmarked,
                is_reader: t.reader_origin_url.is_some(),
            }
        })
        .collect();

    ChromeSyncPayload {
        tabs: tab_dtos,
        active_tab_id,
        sidebar_open: persisted.sidebar_open,
        sidebar_width: persisted.sidebar_width,
        find_open,
        overlay_open,
        theme: persisted.settings.theme.clone(),
        accent: persisted.settings.accent.clone(),
        ai_provider: persisted.settings.ai_provider.clone(),
        active_model: active_model_label(&persisted.settings),
        selection_pill: persisted.settings.selection_pill,
        include_page_context: persisted.settings.include_page_context,
        is_maximized,
        downloads: persisted.downloads.iter().take(15).cloned().collect(),
    }
}

pub fn build_internal_payload(
    persisted: &PersistedState,
    reader_article: Option<ReaderArticle>,
) -> InternalPagePayload {
    InternalPagePayload {
        settings: persisted.settings.clone(),
        bookmarks: persisted.bookmarks.clone(),
        history: persisted.history.clone(),
        speed_dial: persisted.speed_dial.clone(),
        downloads: persisted.downloads.clone(),
        reader_article,
    }
}
