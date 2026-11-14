#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ai;
mod browser;
mod config;
mod protocol;
mod win32;

use crate::ai::{AiStreamEvent, ChatMessage, PageContext};
use crate::browser::{
    AppEvent, CHROME_BASE_HEIGHT, FIND_BAR_HEIGHT, FrontendCommand, MAX_SIDEBAR_WIDTH,
    MIN_SIDEBAR_WIDTH, ReaderArticle, Tab, build_chrome_payload, build_internal_payload,
};
use crate::config::{
    Bookmark, DownloadItem, HistoryItem, OmniboxAction, PersistedState, SpeedDialTile, load_state,
    resolve_omnibox_input, save_state, to_display_url, to_internal_arthos_url, webview_profile_dir,
};
use chrono::Utc;
use std::collections::HashMap;
use tao::{
    dpi::{LogicalPosition, LogicalSize, PhysicalSize},
    event::{ElementState, Event, MouseButton, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    window::{CursorIcon, ResizeDirection, Window, WindowBuilder},
};
use wry::{NewWindowResponse, PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder};

const CONTENT_BRIDGE_JS: &str = include_str!("../ui/content_bridge.js");

fn build_child_webview(builder: WebViewBuilder<'_>, window: &Window) -> wry::Result<WebView> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        builder.build_as_child(window)
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        builder.build_as_child(window)
    }
}

#[derive(Debug, Clone, Copy)]
enum HitEdge {
    Client,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

fn hit_test(size: PhysicalSize<u32>, x: i32, y: i32, scale: f64) -> HitEdge {
    let inset = (6.0 * scale) as i32;
    let w = size.width as i32;
    let h = size.height as i32;
    let left = x < inset;
    let right = x >= w - inset;
    let top = y < inset;
    let bottom = y >= h - inset;

    match (left, right, top, bottom) {
        (true, _, true, _) => HitEdge::TopLeft,
        (_, true, true, _) => HitEdge::TopRight,
        (true, _, _, true) => HitEdge::BottomLeft,
        (_, true, _, true) => HitEdge::BottomRight,
        (true, _, _, _) => HitEdge::Left,
        (_, true, _, _) => HitEdge::Right,
        (_, _, true, _) => HitEdge::Top,
        (_, _, _, true) => HitEdge::Bottom,
        _ => HitEdge::Client,
    }
}

impl HitEdge {
    fn cursor(&self) -> CursorIcon {
        match self {
            HitEdge::Left => CursorIcon::WResize,
            HitEdge::Right => CursorIcon::EResize,
            HitEdge::Top => CursorIcon::NResize,
            HitEdge::Bottom => CursorIcon::SResize,
            HitEdge::TopLeft => CursorIcon::NwResize,
            HitEdge::TopRight => CursorIcon::NeResize,
            HitEdge::BottomLeft => CursorIcon::SwResize,
            HitEdge::BottomRight => CursorIcon::SeResize,
            HitEdge::Client => CursorIcon::Default,
        }
    }

    fn start_resize(&self, window: &Window) {
        let dir = match self {
            HitEdge::Left => Some(ResizeDirection::West),
            HitEdge::Right => Some(ResizeDirection::East),
            HitEdge::Top => Some(ResizeDirection::North),
            HitEdge::Bottom => Some(ResizeDirection::South),
            HitEdge::TopLeft => Some(ResizeDirection::NorthWest),
            HitEdge::TopRight => Some(ResizeDirection::NorthEast),
            HitEdge::BottomLeft => Some(ResizeDirection::SouthWest),
            HitEdge::BottomRight => Some(ResizeDirection::SouthEast),
            HitEdge::Client => None,
        };
        if let Some(d) = dir {
            let _ = window.drag_resize_window(d);
        }
    }
}

struct BrowserApp {
    window: Window,
    proxy: EventLoopProxy<AppEvent>,
    tokio_rt: tokio::runtime::Runtime,
    http_client: reqwest::Client,
    web_context: WebContext,
    chrome_webview: WebView,
    sidebar_webview: WebView,
    overlay_webview: WebView,
    overlay_hwnd: isize,
    tabs: Vec<Tab>,
    active_tab_id: u64,
    next_tab_id: u64,
    persisted: PersistedState,
    find_open: bool,
    overlay_open: bool,
    last_reader_article: Option<ReaderArticle>,
    active_ai_abort: Option<tokio::task::AbortHandle>,
    pending_downloads: HashMap<String, String>,
    cursor_pos: (i32, i32),
}

impl BrowserApp {
    fn new(event_loop: &tao::event_loop::EventLoop<AppEvent>) -> Self {
        let persisted = load_state();
        let dark = persisted.settings.theme != "light";

        let window = WindowBuilder::new()
            .with_title("Arthos")
            .with_inner_size(LogicalSize::new(1360, 860))
            .with_min_inner_size(LogicalSize::new(720, 480))
            .with_decorations(false)
            .build(event_loop)
            .expect("Failed to create main window");

        win32::polish_window(&window, dark);

        let proxy = event_loop.create_proxy();
        let tokio_rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
        let http_client = ai::build_http_client();
        let mut web_context = WebContext::new(Some(webview_profile_dir()));
        let bg_rgba = if dark { (16, 19, 26, 255) } else { (233, 236, 243, 255) };

        let chrome_webview = Self::create_chrome_webview(&window, &mut web_context, bg_rgba, proxy.clone());
        let sidebar_webview = Self::create_sidebar_webview(&window, &mut web_context, bg_rgba, proxy.clone());
        let overlay_webview = Self::create_overlay_webview(&window, &mut web_context, proxy.clone());
        let overlay_hwnd = win32::top_child_hwnd(&window);
        let _ = overlay_webview.set_visible(false);

        win32::apply_webview_theme(&chrome_webview, dark);
        win32::apply_webview_theme(&sidebar_webview, dark);
        win32::apply_webview_theme(&overlay_webview, dark);

        let mut app = Self {
            window,
            proxy: proxy.clone(),
            tokio_rt,
            http_client,
            web_context,
            chrome_webview,
            sidebar_webview,
            overlay_webview,
            overlay_hwnd,
            tabs: Vec::new(),
            active_tab_id: 0,
            next_tab_id: 1,
            persisted,
            find_open: false,
            overlay_open: false,
            last_reader_article: None,
            active_ai_abort: None,
            pending_downloads: HashMap::new(),
            cursor_pos: (0, 0),
        };

        let _ = app.sidebar_webview.set_visible(app.persisted.sidebar_open);

        // Open initial tabs
        let initial_urls: Vec<String> =
            if app.persisted.settings.restore_session && !app.persisted.open_tabs.is_empty() {
                app.persisted.open_tabs.clone()
            } else {
                vec![app.persisted.settings.homepage.clone()]
            };

        for url in initial_urls {
            let internal = to_internal_arthos_url(&url);
            app.create_tab(&internal, false, true);
        }

        if let Some(tab) = app.tabs.get(app.persisted.active_tab_index) {
            let id = tab.id;
            app.switch_tab(id);
        } else if let Some(first) = app.tabs.first() {
            let id = first.id;
            app.switch_tab(id);
        }

        app.layout_webviews();
        app.sync_all_ui();
        app
    }

    fn create_chrome_webview(
        window: &Window,
        web_context: &mut WebContext,
        bg_rgba: (u8, u8, u8, u8),
        proxy: EventLoopProxy<AppEvent>,
    ) -> WebView {
        let size = window.inner_size().to_logical::<u32>(window.scale_factor());
        let builder = WebViewBuilder::new_with_web_context(web_context)
            .with_background_color(bg_rgba)
            .with_bounds(Rect {
                position: LogicalPosition::new(0, 0).into(),
                size: LogicalSize::new(size.width.max(300), CHROME_BASE_HEIGHT).into(),
            })
            .with_custom_protocol("arthos".into(), |_, req| {
                protocol::handle_arthos_protocol(req)
            })
            .with_ipc_handler(move |req| {
                let _ = proxy.send_event(AppEvent::IpcMessage(req.body().to_string()));
            })
            .with_url("arthos://localhost/chrome.html");
        build_child_webview(builder, window).expect("Failed to create chrome webview")
    }

    fn create_sidebar_webview(
        window: &Window,
        web_context: &mut WebContext,
        bg_rgba: (u8, u8, u8, u8),
        proxy: EventLoopProxy<AppEvent>,
    ) -> WebView {
        let builder = WebViewBuilder::new_with_web_context(web_context)
            .with_background_color(bg_rgba)
            .with_bounds(Rect {
                position: LogicalPosition::new(0, CHROME_BASE_HEIGHT).into(),
                size: LogicalSize::new(390, 600).into(),
            })
            .with_custom_protocol("arthos".into(), |_, req| {
                protocol::handle_arthos_protocol(req)
            })
            .with_ipc_handler(move |req| {
                let _ = proxy.send_event(AppEvent::IpcMessage(req.body().to_string()));
            })
            .with_url("arthos://localhost/sidebar.html");
        build_child_webview(builder, window).expect("Failed to create sidebar webview")
    }

    fn create_overlay_webview(
        window: &Window,
        web_context: &mut WebContext,
        proxy: EventLoopProxy<AppEvent>,
    ) -> WebView {
        let size = window.inner_size().to_logical::<u32>(window.scale_factor());
        let builder = WebViewBuilder::new_with_web_context(web_context)
            .with_transparent(true)
            .with_bounds(Rect {
                position: LogicalPosition::new(0, CHROME_BASE_HEIGHT).into(),
                size: LogicalSize::new(size.width.max(300), size.height.max(300)).into(),
            })
            .with_custom_protocol("arthos".into(), |_, req| {
                protocol::handle_arthos_protocol(req)
            })
            .with_ipc_handler(move |req| {
                let _ = proxy.send_event(AppEvent::IpcMessage(req.body().to_string()));
            })
            .with_url("arthos://localhost/overlay.html");
        build_child_webview(builder, window).expect("Failed to create overlay webview")
    }

    fn border_pad(&self) -> u32 {
        if self.window.is_maximized() { 0 } else { 2 }
    }

    fn content_rect(&self) -> Rect {
        let size = self
            .window
            .inner_size()
            .to_logical::<u32>(self.window.scale_factor());
        let pad = self.border_pad();
        let chrome_h = CHROME_BASE_HEIGHT + if self.find_open { FIND_BAR_HEIGHT } else { 0 };
        let sidebar_w = if self.persisted.sidebar_open {
            self.persisted
                .sidebar_width
                .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH)
                .min(size.width.saturating_sub(320))
        } else {
            0
        };
        let avail_w = size.width.saturating_sub(pad * 2 + sidebar_w).max(120);
        let avail_h = size.height.saturating_sub(pad * 2 + chrome_h).max(120);
        Rect {
            position: LogicalPosition::new(pad, pad + chrome_h).into(),
            size: LogicalSize::new(avail_w, avail_h).into(),
        }
    }

    fn layout_webviews(&self) {
        let size = self
            .window
            .inner_size()
            .to_logical::<u32>(self.window.scale_factor());
        let pad = self.border_pad();
        let chrome_h = CHROME_BASE_HEIGHT + if self.find_open { FIND_BAR_HEIGHT } else { 0 };
        let inner_w = size.width.saturating_sub(pad * 2).max(200);
        let inner_h = size.height.saturating_sub(pad * 2).max(200);

        let _ = self.chrome_webview.set_bounds(Rect {
            position: LogicalPosition::new(pad, pad).into(),
            size: LogicalSize::new(inner_w, chrome_h).into(),
        });

        let sidebar_w = if self.persisted.sidebar_open {
            self.persisted
                .sidebar_width
                .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH)
                .min(inner_w.saturating_sub(320))
        } else {
            0
        };

        let content_rect = self.content_rect();
        for tab in &self.tabs {
            let _ = tab.webview.set_bounds(content_rect);
            let _ = tab.webview.set_visible(tab.id == self.active_tab_id);
        }

        if self.persisted.sidebar_open {
            let _ = self.sidebar_webview.set_bounds(Rect {
                position: LogicalPosition::new(pad + inner_w.saturating_sub(sidebar_w), pad + chrome_h)
                    .into(),
                size: LogicalSize::new(sidebar_w, inner_h.saturating_sub(chrome_h)).into(),
            });
            let _ = self.sidebar_webview.set_visible(true);
        } else {
            let _ = self.sidebar_webview.set_visible(false);
        }

        let _ = self.overlay_webview.set_bounds(Rect {
            position: LogicalPosition::new(pad, pad + chrome_h).into(),
            size: LogicalSize::new(inner_w, inner_h.saturating_sub(chrome_h)).into(),
        });
        let _ = self.overlay_webview.set_visible(self.overlay_open);
    }

    fn create_tab(&mut self, url: &str, incognito: bool, activate: bool) -> u64 {
        let tab_id = self.next_tab_id;
        self.next_tab_id += 1;

        let target_url = to_internal_arthos_url(url);
        let rect = self.content_rect();

        let p_ipc = self.proxy.clone();
        let p_title = self.proxy.clone();
        let p_load = self.proxy.clone();
        let p_new = self.proxy.clone();
        let p_dl_start = self.proxy.clone();
        let p_dl_done = self.proxy.clone();

        let dark_bg = if self.persisted.settings.theme != "light" {
            (11, 13, 18, 255)
        } else {
            (245, 246, 250, 255)
        };
        let builder = if incognito {
            WebViewBuilder::new().with_incognito(true)
        } else {
            WebViewBuilder::new_with_web_context(&mut self.web_context)
        }
        .with_background_color(dark_bg)
        .with_bounds(rect)
        .with_visible(activate)
        .with_custom_protocol("arthos".into(), |_, req| {
            protocol::handle_arthos_protocol(req)
        })
        .with_initialization_script_for_main_only(CONTENT_BRIDGE_JS, true)
        .with_ipc_handler(move |req| {
            let _ = p_ipc.send_event(AppEvent::IpcMessage(req.body().to_string()));
        })
        .with_document_title_changed_handler(move |title| {
            let _ = p_title.send_event(AppEvent::TabTitleChanged { tab_id, title });
        })
        .with_on_page_load_handler(move |ev, url| {
            let finished = matches!(ev, PageLoadEvent::Finished);
            let _ = p_load.send_event(AppEvent::TabPageLoad {
                tab_id,
                url,
                finished,
            });
        })
        .with_new_window_req_handler(move |url, _| {
            let _ = p_new.send_event(AppEvent::OpenNewTabUrl { url, incognito });
            NewWindowResponse::Deny
        })
        .with_download_started_handler(move |url, dest| {
            let filename = dest
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "download".to_string());
            let path = dest.to_string_lossy().to_string();
            let id = format!("dl-{}", Utc::now().timestamp_millis());
            let _ = p_dl_start.send_event(AppEvent::DownloadStarted {
                id,
                url,
                filename,
                path,
            });
            true
        })
        .with_download_completed_handler(move |url, path, success| {
            let _ = p_dl_done.send_event(AppEvent::DownloadCompleted {
                url,
                path: path.map(|p| p.to_string_lossy().to_string()),
                success,
            });
        })
        .with_url(&target_url);

        let webview = build_child_webview(builder, &self.window).expect("Failed to build tab webview");
        win32::apply_webview_theme(&webview, self.persisted.settings.theme != "light");
        win32::bring_hwnd_to_top(self.overlay_hwnd);

        self.tabs.push(Tab {
            id: tab_id,
            title: if incognito {
                "Private Tab".to_string()
            } else {
                "New Tab".to_string()
            },
            url: target_url,
            loading: true,
            incognito,
            zoom: 1.0,
            reader_origin_url: None,
            webview,
        });

        if activate {
            self.switch_tab(tab_id);
        }
        self.persist_open_tabs();
        self.sync_chrome();
        tab_id
    }

    fn switch_tab(&mut self, id: u64) {
        if !self.tabs.iter().any(|t| t.id == id) {
            return;
        }
        self.active_tab_id = id;
        for (idx, tab) in self.tabs.iter().enumerate() {
            let visible = tab.id == id;
            let _ = tab.webview.set_visible(visible);
            if visible {
                self.persisted.active_tab_index = idx;
                let _ = tab.webview.focus();
            }
        }
        self.sync_all_ui();
        save_state(&self.persisted);
    }

    fn close_tab(&mut self, target_id: u64) {
        let id = if target_id == 0 {
            self.active_tab_id
        } else {
            target_id
        };
        let Some(pos) = self.tabs.iter().position(|t| t.id == id) else {
            return;
        };
        self.tabs.remove(pos);

        if self.tabs.is_empty() {
            let home = self.persisted.settings.homepage.clone();
            self.create_tab(&home, false, true);
            return;
        }

        if self.active_tab_id == id {
            let next_idx = pos.min(self.tabs.len() - 1);
            let next_id = self.tabs[next_idx].id;
            self.switch_tab(next_id);
        } else {
            self.persist_open_tabs();
            self.sync_chrome();
        }
    }

    fn active_tab(&self) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == self.active_tab_id)
    }

    fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        let id = self.active_tab_id;
        self.tabs.iter_mut().find(|t| t.id == id)
    }

    fn persist_open_tabs(&mut self) {
        self.persisted.open_tabs = self
            .tabs
            .iter()
            .filter(|t| !t.incognito)
            .map(|t| to_display_url(&t.url))
            .collect();
        save_state(&self.persisted);
    }

    fn sync_chrome(&self) {
        let payload = build_chrome_payload(
            &self.tabs,
            self.active_tab_id,
            &self.persisted,
            self.find_open,
            self.overlay_open,
            self.window.is_maximized(),
        );
        if let Ok(json) = serde_json::to_string(&payload) {
            let js = format!("window.__arthosUpdate && window.__arthosUpdate({json});");
            let _ = self.chrome_webview.evaluate_script(&js);
            let sjs = format!("window.__arthosSidebarState && window.__arthosSidebarState({json});");
            let _ = self.sidebar_webview.evaluate_script(&sjs);
        }
    }

    fn sync_internal_active_tab(&self) {
        if let Some(tab) = self.active_tab() {
            let disp = to_display_url(&tab.url);
            if disp.starts_with("arthos://") {
                let payload = build_internal_payload(&self.persisted, self.last_reader_article.clone());
                if let Ok(json) = serde_json::to_string(&payload) {
                    let js = format!("window.__arthosPageData && window.__arthosPageData({json});");
                    let _ = tab.webview.evaluate_script(&js);
                }
            }
            let pill_js = format!(
                "window.__arthosSelectionPillEnabled = {};",
                self.persisted.settings.selection_pill
            );
            let _ = tab.webview.evaluate_script(&pill_js);
        }
    }

    fn sync_all_ui(&self) {
        self.sync_chrome();
        self.sync_internal_active_tab();
    }

    fn emit_sidebar_event(&self, event: serde_json::Value) {
        if let Ok(json) = serde_json::to_string(&event) {
            let js = format!("window.__arthosSidebarEvent && window.__arthosSidebarEvent({json});");
            let _ = self.sidebar_webview.evaluate_script(&js);
        }
    }

    fn emit_toast(&self, message: &str) {
        if let Ok(json) = serde_json::to_string(message) {
            let js = format!("window.__arthosToast && window.__arthosToast({json});");
            let _ = self.chrome_webview.evaluate_script(&js);
        }
    }

    fn navigate_active(&mut self, input: &str) {
        match resolve_omnibox_input(input, &self.persisted.settings.search_engine) {
            OmniboxAction::Navigate(url) => {
                if let Some(tab) = self.active_tab_mut() {
                    tab.reader_origin_url = None;
                    tab.url = url.clone();
                    tab.loading = true;
                    let _ = tab.webview.load_url(&url);
                }
                self.persist_open_tabs();
                self.sync_chrome();
            }
            OmniboxAction::AskAi(question) => {
                self.persisted.sidebar_open = true;
                self.layout_webviews();
                self.sync_chrome();
                let _ = self.sidebar_webview.focus();
                self.emit_sidebar_event(serde_json::json!({
                    "type": "prompt",
                    "text": question
                }));
            }
        }
    }

    fn start_ai_request(&mut self, request_id: String, messages: Vec<ChatMessage>, include_page: bool) {
        if let Some(handle) = self.active_ai_abort.take() {
            handle.abort();
        }

        if include_page {
            if let Some(tab) = self.active_tab() {
                let disp = to_display_url(&tab.url);
                if !disp.starts_with("arthos://") || disp == "arthos://reader" {
                    let proxy = self.proxy.clone();
                    let req_id = request_id.clone();
                    let msgs = messages.clone();
                    let res = tab.webview.evaluate_script_with_callback(
                        "window.__arthosExtractContext ? window.__arthosExtractContext() : null",
                        move |raw| {
                            let ctx = parse_eval_json::<PageContext>(&raw);
                            let _ = proxy.send_event(AppEvent::AiContextReady {
                                request_id: req_id.clone(),
                                messages: msgs.clone(),
                                context: ctx,
                            });
                        },
                    );
                    if res.is_ok() {
                        return;
                    }
                }
            }
        }

        self.spawn_ai_stream(request_id, messages, None);
    }

    fn spawn_ai_stream(
        &mut self,
        request_id: String,
        messages: Vec<ChatMessage>,
        context: Option<PageContext>,
    ) {
        let settings = self.persisted.settings.clone();
        let client = self.http_client.clone();
        let proxy = self.proxy.clone();

        let handle = self
            .tokio_rt
            .spawn(async move {
                ai::run_ai_request(client, request_id, settings, messages, context, |ev| {
                    let _ = proxy.send_event(AppEvent::AiStream(ev));
                })
                .await;
            })
            .abort_handle();

        self.active_ai_abort = Some(handle);
    }

    fn open_overlay(&mut self, open: Option<bool>, mode: Option<String>, query: Option<String>) {
        let next = open.unwrap_or(!self.overlay_open);
        self.overlay_open = next;
        if next {
            win32::bring_hwnd_to_top(self.overlay_hwnd);
        }
        self.layout_webviews();
        if next {
            let payload = serde_json::json!({
                "type": "open",
                "mode": mode.unwrap_or_else(|| "command".to_string()),
                "query": query.unwrap_or_default(),
                "tabs": self.tabs.iter().map(|t| serde_json::json!({
                    "id": t.id,
                    "title": t.title,
                    "url": to_display_url(&t.url)
                })).collect::<Vec<_>>(),
                "bookmarks": self.persisted.bookmarks,
                "history": self.persisted.history.iter().take(30).collect::<Vec<_>>(),
                "theme": self.persisted.settings.theme,
                "accent": self.persisted.settings.accent
            });
            if let Ok(json) = serde_json::to_string(&payload) {
                let js = format!("window.__arthosOverlayEvent && window.__arthosOverlayEvent({json});");
                let _ = self.overlay_webview.evaluate_script(&js);
            }
            let _ = self.overlay_webview.focus();
        } else if let Some(tab) = self.active_tab() {
            let _ = tab.webview.focus();
        }
        self.sync_chrome();
    }

    fn handle_command(&mut self, cmd: FrontendCommand) {
        match cmd {
            FrontendCommand::WindowMinimize => self.window.set_minimized(true),
            FrontendCommand::WindowMaximize => {
                self.window.set_maximized(!self.window.is_maximized());
                self.layout_webviews();
                self.sync_chrome();
            }
            FrontendCommand::WindowClose => std::process::exit(0),
            FrontendCommand::WindowDrag => {
                let _ = self.window.drag_window();
            }
            FrontendCommand::NewTab { url, incognito } => {
                let target = url.unwrap_or_else(|| self.persisted.settings.homepage.clone());
                self.create_tab(&target, incognito.unwrap_or(false), true);
            }
            FrontendCommand::SwitchTab { id } => self.switch_tab(id),
            FrontendCommand::CloseTab { id } => self.close_tab(id),
            FrontendCommand::NextTab => {
                if self.tabs.len() > 1 {
                    if let Some(pos) = self.tabs.iter().position(|t| t.id == self.active_tab_id) {
                        let next_id = self.tabs[(pos + 1) % self.tabs.len()].id;
                        self.switch_tab(next_id);
                    }
                }
            }
            FrontendCommand::PrevTab => {
                if self.tabs.len() > 1 {
                    if let Some(pos) = self.tabs.iter().position(|t| t.id == self.active_tab_id) {
                        let prev_idx = if pos == 0 { self.tabs.len() - 1 } else { pos - 1 };
                        let prev_id = self.tabs[prev_idx].id;
                        self.switch_tab(prev_id);
                    }
                }
            }
            FrontendCommand::Navigate { input } => self.navigate_active(&input),
            FrontendCommand::NavBack => {
                if let Some(tab) = self.active_tab() {
                    let _ = tab.webview.evaluate_script("history.back()");
                }
            }
            FrontendCommand::NavForward => {
                if let Some(tab) = self.active_tab() {
                    let _ = tab.webview.evaluate_script("history.forward()");
                }
            }
            FrontendCommand::NavReload => {
                if let Some(tab) = self.active_tab() {
                    let _ = tab.webview.evaluate_script("location.reload()");
                }
            }
            FrontendCommand::NavHome => {
                let home = self.persisted.settings.homepage.clone();
                self.navigate_active(&home);
            }
            FrontendCommand::ToggleBookmark => {
                if let Some(tab) = self.active_tab() {
                    let disp = to_display_url(&tab.url);
                    if let Some(pos) = self
                        .persisted
                        .bookmarks
                        .iter()
                        .position(|b| b.url == tab.url || b.url == disp)
                    {
                        self.persisted.bookmarks.remove(pos);
                        self.emit_toast("Bookmark removed");
                    } else {
                        self.persisted.bookmarks.insert(
                            0,
                            Bookmark {
                                id: format!("bm-{}", Utc::now().timestamp_millis()),
                                title: if tab.title.is_empty() {
                                    disp.clone()
                                } else {
                                    tab.title.clone()
                                },
                                url: disp,
                                created_at: Utc::now(),
                            },
                        );
                        self.emit_toast("Bookmarked");
                    }
                    save_state(&self.persisted);
                    self.sync_all_ui();
                }
            }
            FrontendCommand::OpenDevtools => {
                if let Some(tab) = self.active_tab() {
                    tab.webview.open_devtools();
                }
            }
            FrontendCommand::ZoomIn => {
                if let Some(tab) = self.active_tab_mut() {
                    tab.zoom = (tab.zoom + 0.1).min(2.5);
                    let _ = tab.webview.zoom(tab.zoom);
                }
                self.sync_chrome();
            }
            FrontendCommand::ZoomOut => {
                if let Some(tab) = self.active_tab_mut() {
                    tab.zoom = (tab.zoom - 0.1).max(0.5);
                    let _ = tab.webview.zoom(tab.zoom);
                }
                self.sync_chrome();
            }
            FrontendCommand::ZoomReset => {
                if let Some(tab) = self.active_tab_mut() {
                    tab.zoom = 1.0;
                    let _ = tab.webview.zoom(1.0);
                }
                self.sync_chrome();
            }
            FrontendCommand::ToggleFindBar { open } => {
                self.find_open = open.unwrap_or(!self.find_open);
                self.layout_webviews();
                self.sync_chrome();
                if self.find_open {
                    let _ = self.chrome_webview.focus();
                    let _ = self
                        .chrome_webview
                        .evaluate_script("window.__arthosFocusFind && window.__arthosFocusFind()");
                } else if let Some(tab) = self.active_tab() {
                    let _ = tab.webview.focus();
                }
            }
            FrontendCommand::FindInPage { query, backward } => {
                if let Some(tab) = self.active_tab() {
                    if let Ok(q_json) = serde_json::to_string(&query) {
                        let back = backward.unwrap_or(false);
                        let js = format!("window.find({q_json}, false, {back}, true, false, false, false);");
                        let _ = tab.webview.evaluate_script(&js);
                    }
                }
            }
            FrontendCommand::ToggleReaderMode => {
                if let Some(tab) = self.active_tab() {
                    if let Some(origin) = tab.reader_origin_url.clone() {
                        if let Some(t) = self.active_tab_mut() {
                            t.reader_origin_url = None;
                            t.url = origin.clone();
                            let _ = t.webview.load_url(&origin);
                        }
                        self.sync_chrome();
                        return;
                    }
                    let proxy = self.proxy.clone();
                    let _ = tab.webview.evaluate_script_with_callback(
                        "window.__arthosExtractReader ? window.__arthosExtractReader() : null",
                        move |raw| {
                            let parsed = parse_eval_json::<ReaderArticle>(&raw)
                                .ok_or_else(|| "Could not extract readable content from this page.".to_string());
                            let _ = proxy.send_event(AppEvent::ReaderExtracted(parsed));
                        },
                    );
                }
            }
            FrontendCommand::ToggleSidebar { open } => {
                self.persisted.sidebar_open = open.unwrap_or(!self.persisted.sidebar_open);
                self.layout_webviews();
                save_state(&self.persisted);
                self.sync_chrome();
                if self.persisted.sidebar_open {
                    let _ = self.sidebar_webview.focus();
                    let _ = self
                        .sidebar_webview
                        .evaluate_script("window.__arthosFocusChat && window.__arthosFocusChat()");
                }
            }
            FrontendCommand::SetSidebarWidth { width } => {
                self.persisted.sidebar_width = width.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
                self.layout_webviews();
                save_state(&self.persisted);
            }
            FrontendCommand::ToggleOverlay { open, mode, query } => {
                self.open_overlay(open, mode, query);
            }
            FrontendCommand::FocusOmnibox => {
                let _ = self.chrome_webview.focus();
                let _ = self
                    .chrome_webview
                    .evaluate_script("window.__arthosFocusOmnibox && window.__arthosFocusOmnibox()");
            }
            FrontendCommand::AiAsk {
                request_id,
                messages,
                include_page,
            } => {
                self.start_ai_request(request_id, messages, include_page);
            }
            FrontendCommand::AiStop => {
                if let Some(handle) = self.active_ai_abort.take() {
                    handle.abort();
                }
            }
            FrontendCommand::AiSetProvider { provider, model } => {
                self.persisted.settings.ai_provider = provider.clone();
                if let Some(m) = model {
                    match provider.as_str() {
                        "openai" => self.persisted.settings.openai_model = m,
                        "anthropic" => self.persisted.settings.anthropic_model = m,
                        "gemini" => self.persisted.settings.gemini_model = m,
                        "ollama" => self.persisted.settings.ollama_model = m,
                        "custom" => self.persisted.settings.custom_model = m,
                        _ => {}
                    }
                }
                save_state(&self.persisted);
                self.sync_all_ui();
            }
            FrontendCommand::AskSelection { text, action } => {
                let prompt = match action.as_str() {
                    "explain" => format!("Explain this selected text clearly:\n\n> {text}"),
                    "summarize" => format!("Summarize this selected text concisely:\n\n> {text}"),
                    _ => format!("Regarding this selected text:\n\n> {text}\n\nWhat are the key insights?"),
                };
                self.persisted.sidebar_open = true;
                self.layout_webviews();
                self.sync_chrome();
                let _ = self.sidebar_webview.focus();
                self.emit_sidebar_event(serde_json::json!({
                    "type": "prompt",
                    "text": prompt
                }));
            }
            FrontendCommand::GrabSelection => {
                if let Some(tab) = self.active_tab() {
                    let proxy = self.proxy.clone();
                    let _ = tab.webview.evaluate_script_with_callback(
                        "(window.getSelection ? window.getSelection().toString().trim() : '')",
                        move |raw| {
                            let sel = serde_json::from_str::<String>(&raw).unwrap_or(raw);
                            let _ = proxy.send_event(AppEvent::SelectionGrabbed(sel));
                        },
                    );
                }
            }
            FrontendCommand::GetState => {
                self.sync_all_ui();
            }
            FrontendCommand::SaveSettings { settings } => {
                let theme_changed = self.persisted.settings.theme != settings.theme;
                self.persisted.settings = settings;
                save_state(&self.persisted);
                if theme_changed {
                    let dark = self.persisted.settings.theme != "light";
                    win32::polish_window(&self.window, dark);
                    win32::apply_webview_theme(&self.chrome_webview, dark);
                    win32::apply_webview_theme(&self.sidebar_webview, dark);
                    win32::apply_webview_theme(&self.overlay_webview, dark);
                    for t in &self.tabs {
                        win32::apply_webview_theme(&t.webview, dark);
                    }
                }
                self.sync_all_ui();
                self.emit_toast("Settings saved");
            }
            FrontendCommand::AddSpeedDial { title, url } => {
                let normalized = match resolve_omnibox_input(&url, &self.persisted.settings.search_engine) {
                    OmniboxAction::Navigate(u) => to_display_url(&u),
                    OmniboxAction::AskAi(_) => return,
                };
                let icon: String = title
                    .split_whitespace()
                    .take(2)
                    .filter_map(|w| w.chars().next())
                    .collect::<String>()
                    .to_uppercase();
                let icon = if icon.is_empty() { "★".into() } else { icon };
                self.persisted.speed_dial.push(SpeedDialTile {
                    title,
                    url: normalized,
                    icon,
                });
                save_state(&self.persisted);
                self.sync_internal_active_tab();
            }
            FrontendCommand::RemoveSpeedDial { url } => {
                self.persisted.speed_dial.retain(|t| t.url != url);
                save_state(&self.persisted);
                self.sync_internal_active_tab();
            }
            FrontendCommand::DeleteBookmark { id } => {
                self.persisted.bookmarks.retain(|b| b.id != id);
                save_state(&self.persisted);
                self.sync_all_ui();
            }
            FrontendCommand::ClearHistory => {
                self.persisted.history.clear();
                save_state(&self.persisted);
                self.sync_internal_active_tab();
                self.emit_toast("History cleared");
            }
            FrontendCommand::ClearBookmarks => {
                self.persisted.bookmarks.clear();
                save_state(&self.persisted);
                self.sync_all_ui();
                self.emit_toast("Bookmarks cleared");
            }
            FrontendCommand::ClearDownloads => {
                self.persisted.downloads.clear();
                save_state(&self.persisted);
                self.sync_all_ui();
            }
            FrontendCommand::DetectOllama => {
                let ollama_url = self.persisted.settings.ollama_url.clone();
                let proxy = self.proxy.clone();
                self.tokio_rt.spawn(async move {
                    let res = ai::fetch_ollama_models(&ollama_url).await;
                    let _ = proxy.send_event(AppEvent::OllamaModelsDetected(res));
                });
            }
            FrontendCommand::OpenExternalTab { url } => {
                let internal = to_internal_arthos_url(&url);
                self.create_tab(&internal, false, true);
            }
            FrontendCommand::ShowToast { message } => {
                self.emit_toast(&message);
            }
        }
    }

    fn handle_app_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::IpcMessage(raw) => {
                if let Ok(cmd) = serde_json::from_str::<FrontendCommand>(&raw) {
                    self.handle_command(cmd);
                }
            }
            AppEvent::TabTitleChanged { tab_id, title } => {
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.title = title;
                }
                self.sync_chrome();
            }
            AppEvent::TabPageLoad {
                tab_id,
                url,
                finished,
            } => {
                let mut record_history = None;
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
                    let is_reader_url = to_display_url(&url) == "arthos://reader";
                    if !is_reader_url && tab.reader_origin_url.is_some() && finished {
                        tab.reader_origin_url = None;
                    }
                    tab.url = url.clone();
                    tab.loading = !finished;

                    if finished && !tab.incognito {
                        let disp = to_display_url(&url);
                        if !disp.starts_with("arthos://") {
                            record_history = Some((tab.title.clone(), disp));
                        }
                    }
                }

                if let Some((title, disp_url)) = record_history {
                    let should_add = self
                        .persisted
                        .history
                        .first()
                        .map(|h| h.url != disp_url)
                        .unwrap_or(true);
                    if should_add {
                        self.persisted.history.insert(
                            0,
                            HistoryItem {
                                id: format!("hist-{}", Utc::now().timestamp_millis()),
                                title: if title.is_empty() {
                                    disp_url.clone()
                                } else {
                                    title
                                },
                                url: disp_url,
                                visited_at: Utc::now(),
                            },
                        );
                        self.persisted.history.truncate(500);
                    }
                }

                if finished {
                    self.persist_open_tabs();
                    if tab_id == self.active_tab_id {
                        self.sync_internal_active_tab();
                    }
                }
                self.sync_chrome();
            }
            AppEvent::OpenNewTabUrl { url, incognito } => {
                self.create_tab(&url, incognito, true);
            }
            AppEvent::DownloadStarted {
                id,
                url,
                filename,
                path,
            } => {
                self.pending_downloads.insert(url.clone(), id.clone());
                self.persisted.downloads.insert(
                    0,
                    DownloadItem {
                        id,
                        filename: filename.clone(),
                        url,
                        path,
                        status: "downloading".to_string(),
                        started_at: Utc::now(),
                    },
                );
                self.persisted.downloads.truncate(100);
                save_state(&self.persisted);
                self.sync_all_ui();
                self.emit_toast(&format!("Downloading {filename}..."));
            }
            AppEvent::DownloadCompleted { url, path, success } => {
                if let Some(id) = self.pending_downloads.remove(&url) {
                    if let Some(item) = self.persisted.downloads.iter_mut().find(|d| d.id == id) {
                        item.status = if success {
                            "completed".to_string()
                        } else {
                            "failed".to_string()
                        };
                        if let Some(p) = path {
                            item.path = p;
                        }
                        let msg = if success {
                            format!("Downloaded {}", item.filename)
                        } else {
                            format!("Download failed: {}", item.filename)
                        };
                        self.emit_toast(&msg);
                    }
                    save_state(&self.persisted);
                    self.sync_all_ui();
                }
            }
            AppEvent::AiContextReady {
                request_id,
                messages,
                context,
            } => {
                self.spawn_ai_stream(request_id, messages, context);
            }
            AppEvent::AiStream(ev) => match ev {
                AiStreamEvent::Chunk { request_id, text } => {
                    self.emit_sidebar_event(serde_json::json!({
                        "type": "chunk",
                        "request_id": request_id,
                        "text": text
                    }));
                }
                AiStreamEvent::Done { request_id } => {
                    self.active_ai_abort = None;
                    self.emit_sidebar_event(serde_json::json!({
                        "type": "done",
                        "request_id": request_id
                    }));
                }
                AiStreamEvent::Error {
                    request_id,
                    message,
                } => {
                    self.active_ai_abort = None;
                    self.emit_sidebar_event(serde_json::json!({
                        "type": "error",
                        "request_id": request_id,
                        "message": message
                    }));
                }
            },
            AppEvent::OllamaModelsDetected(res) => {
                if let Some(tab) = self.active_tab() {
                    let payload = match res {
                        Ok(models) => serde_json::json!({ "ok": true, "models": models }),
                        Err(err) => serde_json::json!({ "ok": false, "error": err }),
                    };
                    if let Ok(json) = serde_json::to_string(&payload) {
                        let js = format!("window.__arthosOllamaModels && window.__arthosOllamaModels({json});");
                        let _ = tab.webview.evaluate_script(&js);
                    }
                }
            }
            AppEvent::ReaderExtracted(res) => match res {
                Ok(article) => {
                    let origin = article.url.clone();
                    self.last_reader_article = Some(article);
                    if let Some(tab) = self.active_tab_mut() {
                        tab.reader_origin_url = Some(origin);
                        let reader_url = "arthos://localhost/reader".to_string();
                        tab.url = reader_url.clone();
                        let _ = tab.webview.load_url(&reader_url);
                    }
                    self.sync_chrome();
                }
                Err(msg) => {
                    self.emit_toast(&msg);
                }
            },
            AppEvent::SelectionGrabbed(text) => {
                if text.trim().is_empty() {
                    self.emit_toast("No text selected on page");
                } else {
                    self.emit_sidebar_event(serde_json::json!({
                        "type": "selection",
                        "text": text
                    }));
                }
            }
        }
    }
}

fn parse_eval_json<T: serde::de::DeserializeOwned>(raw: &str) -> Option<T> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "null" || trimmed == "undefined" {
        return None;
    }
    if let Ok(val) = serde_json::from_str::<T>(trimmed) {
        return Some(val);
    }
    if let Ok(inner_str) = serde_json::from_str::<String>(trimmed) {
        if let Ok(val) = serde_json::from_str::<T>(&inner_str) {
            return Some(val);
        }
    }
    None
}

fn main() -> wry::Result<()> {
    let event_loop = EventLoopBuilder::<AppEvent>::with_user_event().build();
    let mut app = BrowserApp::new(&event_loop);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::UserEvent(app_event) => {
                app.handle_app_event(app_event);
            }
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::Resized(_) => {
                    app.layout_webviews();
                    app.sync_chrome();
                }
                WindowEvent::CursorMoved { position, .. } => {
                    app.cursor_pos = (position.x as i32, position.y as i32);
                    if !app.window.is_maximized() {
                        let edge = hit_test(
                            app.window.inner_size(),
                            app.cursor_pos.0,
                            app.cursor_pos.1,
                            app.window.scale_factor(),
                        );
                        app.window.set_cursor_icon(edge.cursor());
                    }
                }
                WindowEvent::MouseInput {
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                    ..
                } => {
                    if !app.window.is_maximized() {
                        let edge = hit_test(
                            app.window.inner_size(),
                            app.cursor_pos.0,
                            app.cursor_pos.1,
                            app.window.scale_factor(),
                        );
                        edge.start_resize(&app.window);
                    }
                }
                WindowEvent::CloseRequested => {
                    app.persist_open_tabs();
                    *control_flow = ControlFlow::Exit;
                }
                _ => {}
            },
            _ => {}
        }
    });
}
