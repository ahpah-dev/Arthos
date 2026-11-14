use std::borrow::Cow;
use wry::http::{Request, Response, header::CONTENT_TYPE};

const THEME_CSS: &str = include_str!("../ui/theme.css");
const CHROME_HTML: &str = include_str!("../ui/chrome.html");
const CHROME_CSS: &str = include_str!("../ui/chrome.css");
const CHROME_JS: &str = include_str!("../ui/chrome.js");
const SIDEBAR_HTML: &str = include_str!("../ui/sidebar.html");
const SIDEBAR_CSS: &str = include_str!("../ui/sidebar.css");
const SIDEBAR_JS: &str = include_str!("../ui/sidebar.js");
const OVERLAY_HTML: &str = include_str!("../ui/overlay.html");
const OVERLAY_CSS: &str = include_str!("../ui/overlay.css");
const OVERLAY_JS: &str = include_str!("../ui/overlay.js");
const NEWTAB_HTML: &str = include_str!("../ui/newtab.html");
const SETTINGS_HTML: &str = include_str!("../ui/settings.html");
const LIBRARY_HTML: &str = include_str!("../ui/library.html");
const READER_HTML: &str = include_str!("../ui/reader.html");

pub fn handle_arthos_protocol(request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let raw_path = request.uri().path();
    let path = raw_path.trim_start_matches('/').to_lowercase();

    let (mime, body): (&str, &[u8]) = match path.as_str() {
        "theme.css" => ("text/css; charset=utf-8", THEME_CSS.as_bytes()),
        "chrome.html" => ("text/html; charset=utf-8", CHROME_HTML.as_bytes()),
        "chrome.css" => ("text/css; charset=utf-8", CHROME_CSS.as_bytes()),
        "chrome.js" => ("application/javascript; charset=utf-8", CHROME_JS.as_bytes()),
        "sidebar.html" => ("text/html; charset=utf-8", SIDEBAR_HTML.as_bytes()),
        "sidebar.css" => ("text/css; charset=utf-8", SIDEBAR_CSS.as_bytes()),
        "sidebar.js" => ("application/javascript; charset=utf-8", SIDEBAR_JS.as_bytes()),
        "overlay.html" => ("text/html; charset=utf-8", OVERLAY_HTML.as_bytes()),
        "overlay.css" => ("text/css; charset=utf-8", OVERLAY_CSS.as_bytes()),
        "overlay.js" => ("application/javascript; charset=utf-8", OVERLAY_JS.as_bytes()),
        "" | "newtab" | "newtab.html" => ("text/html; charset=utf-8", NEWTAB_HTML.as_bytes()),
        "settings" | "settings.html" => ("text/html; charset=utf-8", SETTINGS_HTML.as_bytes()),
        "library" | "history" | "bookmarks" | "downloads" | "passwords" | "vault" | "library.html" => {
            ("text/html; charset=utf-8", LIBRARY_HTML.as_bytes())
        }
        "reader" | "reader.html" => ("text/html; charset=utf-8", READER_HTML.as_bytes()),
        _ => ("text/html; charset=utf-8", NEWTAB_HTML.as_bytes()),
    };

    Response::builder()
        .status(200)
        .header(CONTENT_TYPE, mime)
        .header("Access-Control-Allow-Origin", "*")
        .body(Cow::Borrowed(body))
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_internal_routes_and_assets() {
        for (path, expected_ct) in [
            ("/newtab", "text/html"),
            ("/settings", "text/html"),
            ("/library", "text/html"),
            ("/reader", "text/html"),
            ("/theme.css", "text/css"),
            ("/chrome.js", "application/javascript"),
        ] {
            let req = Request::builder()
                .uri(format!("arthos://localhost{path}"))
                .body(Vec::new())
                .unwrap();
            let resp = handle_arthos_protocol(req);
            assert_eq!(resp.status(), 200);
            let ct = resp.headers().get(CONTENT_TYPE).unwrap().to_str().unwrap();
            assert!(ct.starts_with(expected_ct), "unexpected ct {ct} for {path}");
            assert!(!resp.body().is_empty());
        }
    }
}
