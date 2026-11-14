use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use url::Url;

use crate::config::{Bookmark, app_data_dir};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultEntry {
    pub id: String,
    pub origin: String,
    pub url: String,
    pub username: String,
    pub password_enc: String, // DPAPI-protected base64 on Windows
    pub source: String,       // "chrome" | "csv" | "arthos"
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct VaultEntryDto {
    pub id: String,
    pub origin: String,
    pub url: String,
    pub username: String,
    pub password: String,
    pub source: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CredentialVault {
    pub entries: Vec<VaultEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncedChromeTab {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChromeSyncReport {
    pub chrome_found: bool,
    pub profiles_scanned: usize,
    pub tabs_found: usize,
    pub tabs_opened: usize,
    pub logins_synced: usize,
    pub logins_skipped_v20: usize,
    pub bookmarks_synced: usize,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub note: String,
}

pub fn vault_file_path() -> PathBuf {
    app_data_dir().join("vault.json")
}

pub fn load_vault() -> CredentialVault {
    let path = vault_file_path();
    match fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(raw.trim_start_matches('\u{feff}')).unwrap_or_default(),
        Err(_) => CredentialVault::default(),
    }
}

pub fn save_vault(vault: &CredentialVault) {
    let path = vault_file_path();
    if let Ok(json) = serde_json::to_string_pretty(vault) {
        let _ = fs::write(path, json);
    }
}

#[cfg(target_os = "windows")]
pub fn protect_secret(plaintext: &str) -> String {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData,
    };

    let bytes = plaintext.as_bytes();
    if bytes.is_empty() {
        return String::new();
    }
    let mut in_blob = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        CryptProtectData(
            &mut in_blob,
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out_blob,
        )
    };
    if ok != 0 && !out_blob.pbData.is_null() {
        let slice = unsafe { std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize) };
        let encoded = format!("dpapi:{}", B64.encode(slice));
        unsafe {
            LocalFree(out_blob.pbData as _);
        }
        encoded
    } else {
        format!("b64:{}", B64.encode(bytes))
    }
}

#[cfg(target_os = "windows")]
pub fn unprotect_secret(encoded: &str) -> String {
    if let Some(rest) = encoded.strip_prefix("dpapi:") {
        if let Ok(raw) = B64.decode(rest) {
            if let Some(plain) = dpapi_unprotect_bytes(&raw) {
                return String::from_utf8_lossy(&plain).into_owned();
            }
        }
        return String::new();
    }
    if let Some(rest) = encoded.strip_prefix("b64:") {
        if let Ok(raw) = B64.decode(rest) {
            return String::from_utf8_lossy(&raw).into_owned();
        }
    }
    encoded.to_string()
}

#[cfg(target_os = "windows")]
fn dpapi_unprotect_bytes(cipher: &[u8]) -> Option<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
    };

    if cipher.is_empty() {
        return None;
    }
    let mut in_blob = CRYPT_INTEGER_BLOB {
        cbData: cipher.len() as u32,
        pbData: cipher.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };
    let ok = unsafe {
        CryptUnprotectData(
            &mut in_blob,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out_blob,
        )
    };
    if ok != 0 && !out_blob.pbData.is_null() {
        let slice = unsafe { std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize) };
        let res = slice.to_vec();
        unsafe {
            LocalFree(out_blob.pbData as _);
        }
        Some(res)
    } else {
        None
    }
}

#[cfg(not(target_os = "windows"))]
pub fn protect_secret(plaintext: &str) -> String {
    format!("b64:{}", B64.encode(plaintext.as_bytes()))
}

#[cfg(not(target_os = "windows"))]
pub fn unprotect_secret(encoded: &str) -> String {
    if let Some(rest) = encoded.strip_prefix("b64:") {
        if let Ok(raw) = B64.decode(rest) {
            return String::from_utf8_lossy(&raw).into_owned();
        }
    }
    encoded.to_string()
}

#[cfg(not(target_os = "windows"))]
fn dpapi_unprotect_bytes(_cipher: &[u8]) -> Option<Vec<u8>> {
    None
}

pub fn normalize_origin(raw_url: &str) -> String {
    if let Ok(u) = Url::parse(raw_url) {
        if let Some(host) = u.host_str() {
            return format!("{}://{}", u.scheme(), host.to_lowercase());
        }
    }
    raw_url.trim().to_lowercase()
}

pub fn vault_dtos(vault: &CredentialVault) -> Vec<VaultEntryDto> {
    vault
        .entries
        .iter()
        .map(|e| VaultEntryDto {
            id: e.id.clone(),
            origin: e.origin.clone(),
            url: e.url.clone(),
            username: e.username.clone(),
            password: unprotect_secret(&e.password_enc),
            source: e.source.clone(),
            updated_at: e.updated_at,
        })
        .collect()
}

pub fn matching_logins_for_url(vault: &CredentialVault, page_url: &str) -> Vec<(String, String)> {
    let origin = normalize_origin(page_url);
    let page_host = Url::parse(page_url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_lowercase()))
        .unwrap_or_default();

    vault
        .entries
        .iter()
        .filter(|e| {
            if e.origin == origin {
                return true;
            }
            if !page_host.is_empty() {
                if let Ok(eu) = Url::parse(&e.url) {
                    if let Some(eh) = eu.host_str() {
                        let ehl = eh.to_lowercase();
                        return ehl == page_host || page_host.ends_with(&format!(".{ehl}"));
                    }
                }
            }
            false
        })
        .filter_map(|e| {
            let pw = unprotect_secret(&e.password_enc);
            if e.username.is_empty() && pw.is_empty() {
                None
            } else {
                Some((e.username.clone(), pw))
            }
        })
        .collect()
}

pub fn upsert_vault_entry(
    vault: &mut CredentialVault,
    url: &str,
    username: &str,
    password: &str,
    source: &str,
) -> bool {
    let origin = normalize_origin(url);
    let enc = protect_secret(password);
    if let Some(existing) = vault
        .entries
        .iter_mut()
        .find(|e| e.origin == origin && e.username == username)
    {
        if existing.password_enc != enc && !password.is_empty() {
            existing.password_enc = enc;
            existing.updated_at = Utc::now();
            return true;
        }
        return false;
    }

    vault.entries.insert(
        0,
        VaultEntry {
            id: format!("pw-{}-{}", Utc::now().timestamp_millis(), vault.entries.len()),
            origin,
            url: url.to_string(),
            username: username.to_string(),
            password_enc: enc,
            source: source.to_string(),
            updated_at: Utc::now(),
        },
    );
    true
}

pub fn chrome_user_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Some(local) = dirs::data_local_dir() {
            let p = local.join("Google").join("Chrome").join("User Data");
            if p.exists() {
                return Some(p);
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(app_sup) = dirs::data_dir() {
            let p = app_sup.join("Google").join("Chrome");
            if p.exists() {
                return Some(p);
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(cfg) = dirs::config_dir() {
            for name in ["google-chrome", "chromium"] {
                let p = cfg.join(name);
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn find_chrome_profiles(user_data: &Path) -> Vec<PathBuf> {
    let mut profiles = Vec::new();
    let default = user_data.join("Default");
    if default.is_dir() {
        profiles.push(default);
    }
    if let Ok(rd) = fs::read_dir(user_data) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                    if name.starts_with("Profile ") {
                        profiles.push(path);
                    }
                }
            }
        }
    }
    profiles
}

fn read_u16_le(b: &[u8], off: usize) -> Option<u16> {
    b.get(off..off + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
}

fn read_u32_le(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn read_i32_le(b: &[u8], off: usize) -> Option<i32> {
    b.get(off..off + 4)
        .map(|s| i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

#[inline]
fn align4(len: usize) -> usize {
    (len + 3) & !3
}

/// Parses Chrome SNSS session/tab binary files (`Tabs_*`, `Current Tabs`, `Session_*`)
/// and extracts the latest navigation `(url, title)` for each tab.
pub fn parse_snss_tabs(data: &[u8]) -> Vec<SyncedChromeTab> {
    if data.len() < 8 || &data[0..4] != b"SNSS" {
        return Vec::new();
    }

    let mut tabs: BTreeMap<i32, (i32, String, String)> = BTreeMap::new();
    let mut closed_tabs: HashSet<i32> = HashSet::new();
    let mut active_indices: BTreeMap<i32, i32> = BTreeMap::new();
    let mut pos = 8usize;

    while pos + 3 <= data.len() {
        let Some(rec_size) = read_u16_le(data, pos) else {
            break;
        };
        let size = rec_size as usize;
        if size == 0 || pos + 2 + size > data.len() {
            break;
        }
        let cmd_id = data[pos + 2];
        let payload = &data[pos + 3..pos + 2 + size];
        pos += 2 + size;

        // Command ID 1 (Tab file) and 6 (Session file): UpdateTabNavigation
        // Layout: [u32 pickle_len][i32 tab_id][i32 index][u32 url_len][url][pad4][u32 title_chars][title_utf16]...
        if (cmd_id == 1 || cmd_id == 6) && payload.len() >= 16 {
            let Some(pickle_len) = read_u32_le(payload, 0) else {
                continue;
            };
            let pickle = &payload[4..payload.len().min(4 + pickle_len as usize)];
            if pickle.len() < 12 {
                continue;
            }
            let Some(tab_id) = read_i32_le(pickle, 0) else {
                continue;
            };
            let Some(nav_idx) = read_i32_le(pickle, 4) else {
                continue;
            };
            let Some(url_len) = read_u32_le(pickle, 8) else {
                continue;
            };
            let url_len = url_len as usize;
            let url_start = 12usize;
            if url_start + url_len > pickle.len() {
                continue;
            }
            let url = String::from_utf8_lossy(&pickle[url_start..url_start + url_len]).into_owned();
            let mut cursor = url_start + align4(url_len);
            let mut title = String::new();
            if let Some(title_chars) = read_u32_le(pickle, cursor) {
                cursor += 4;
                let byte_len = (title_chars as usize).saturating_mul(2);
                if cursor + byte_len <= pickle.len() {
                    let utf16: Vec<u16> = pickle[cursor..cursor + byte_len]
                        .chunks_exact(2)
                        .map(|c| u16::from_le_bytes([c[0], c[1]]))
                        .collect();
                    title = String::from_utf16_lossy(&utf16);
                }
            }

            if !url.is_empty() {
                let target_idx = active_indices.get(&tab_id).copied();
                let should_update = match tabs.get(&tab_id) {
                    None => true,
                    Some((prev_idx, _, _)) => match target_idx {
                        Some(sel) => nav_idx == sel || (*prev_idx != sel && nav_idx >= *prev_idx),
                        None => nav_idx >= *prev_idx,
                    },
                };
                if should_update {
                    tabs.insert(tab_id, (nav_idx, url, title));
                }
            }
        } else if cmd_id == 7 && payload.len() >= 8 {
            // SetSelectedNavigationIndex: [i32 tab_id][i32 index]
            if let (Some(tab_id), Some(sel)) = (read_i32_le(payload, 0), read_i32_le(payload, 4)) {
                active_indices.insert(tab_id, sel);
            }
        } else if (cmd_id == 16 || cmd_id == 8) && payload.len() >= 4 {
            // TabClosed
            if let Some(tab_id) = read_i32_le(payload, 0) {
                closed_tabs.insert(tab_id);
            }
        }
    }

    let mut seen_urls = HashSet::new();
    let mut out = Vec::new();
    for (tab_id, (_, url, title)) in tabs {
        if closed_tabs.contains(&tab_id) {
            continue;
        }
        if !url.starts_with("http://") && !url.starts_with("https://") {
            continue;
        }
        if seen_urls.insert(url.clone()) {
            out.push(SyncedChromeTab { url, title });
        }
    }
    out
}

fn read_chrome_open_tabs(profile: &Path) -> Vec<SyncedChromeTab> {
    let mut candidates: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    let sessions_dir = profile.join("Sessions");
    if let Ok(rd) = fs::read_dir(&sessions_dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                if name.starts_with("Tabs_") {
                    let mtime = p
                        .metadata()
                        .and_then(|m| m.modified())
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    candidates.push((mtime, p));
                }
            }
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0));

    for (_, p) in candidates {
        if let Ok(bytes) = fs::read(&p) {
            let parsed = parse_snss_tabs(&bytes);
            if !parsed.is_empty() {
                return parsed;
            }
        }
    }

    for legacy in ["Current Tabs", "Last Tabs", "Current Session"] {
        let p = profile.join(legacy);
        if let Ok(bytes) = fs::read(&p) {
            let parsed = parse_snss_tabs(&bytes);
            if !parsed.is_empty() {
                return parsed;
            }
        }
    }
    Vec::new()
}

fn collect_bookmarks_from_node(node: &Value, out: &mut Vec<(String, String)>) {
    if node["type"].as_str() == Some("url") {
        if let Some(url) = node["url"].as_str() {
            if url.starts_with("http://") || url.starts_with("https://") {
                let title = node["name"].as_str().unwrap_or(url).to_string();
                out.push((title, url.to_string()));
            }
        }
    }
    if let Some(children) = node["children"].as_array() {
        for child in children {
            collect_bookmarks_from_node(child, out);
        }
    }
}

fn read_chrome_bookmarks(profile: &Path) -> Vec<(String, String)> {
    let path = profile.join("Bookmarks");
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(val) = serde_json::from_str::<Value>(&raw) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(roots) = val["roots"].as_object() {
        for (_, node) in roots {
            collect_bookmarks_from_node(node, &mut out);
        }
    }
    out
}

fn read_chrome_master_key(user_data: &Path) -> Option<Vec<u8>> {
    let local_state = fs::read_to_string(user_data.join("Local State")).ok()?;
    let val: Value = serde_json::from_str(&local_state).ok()?;
    let enc_b64 = val["os_crypt"]["encrypted_key"].as_str()?;
    let decoded = B64.decode(enc_b64).ok()?;
    let dpapi_blob = decoded.strip_prefix(b"DPAPI")?;
    dpapi_unprotect_bytes(dpapi_blob)
}

fn decrypt_chrome_password_blob(blob: &[u8], master_key: Option<&[u8]>) -> Result<String, &'static str> {
    if blob.is_empty() {
        return Ok(String::new());
    }
    if blob.starts_with(b"v20") {
        return Err("v20");
    }
    if (blob.starts_with(b"v10") || blob.starts_with(b"v11")) && blob.len() > 15 {
        let Some(key) = master_key else {
            return Err("no_key");
        };
        let Ok(cipher) = Aes256Gcm::new_from_slice(key) else {
            return Err("bad_key");
        };
        let nonce = Nonce::from_slice(&blob[3..15]);
        let plaintext = cipher.decrypt(nonce, &blob[15..]).map_err(|_| "decrypt_fail")?;
        return Ok(String::from_utf8_lossy(&plaintext).into_owned());
    }
    if let Some(plain) = dpapi_unprotect_bytes(blob) {
        return Ok(String::from_utf8_lossy(&plain).into_owned());
    }
    Err("unsupported")
}

fn read_chrome_logins(
    profile: &Path,
    master_key: Option<&[u8]>,
) -> (Vec<(String, String, String)>, usize) {
    let login_db = profile.join("Login Data");
    if !login_db.exists() {
        return (Vec::new(), 0);
    }

    let tmp_db = std::env::temp_dir().join(format!(
        "arthos_chrome_logins_{}_{}.db",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    if fs::copy(&login_db, &tmp_db).is_err() {
        return (Vec::new(), 0);
    }

    let mut out = Vec::new();
    let mut skipped_v20 = 0usize;

    if let Ok(conn) = Connection::open(&tmp_db) {
        if let Ok(mut stmt) = conn.prepare(
            "SELECT origin_url, username_value, password_value FROM logins WHERE blacklisted_by_user = 0",
        ) {
            if let Ok(rows) = stmt.query_map([], |row| {
                let url: String = row.get(0)?;
                let user: String = row.get(1)?;
                let blob: Vec<u8> = row.get(2)?;
                Ok((url, user, blob))
            }) {
                for row in rows.flatten() {
                    let (url, user, blob) = row;
                    if url.is_empty() || user.is_empty() {
                        continue;
                    }
                    match decrypt_chrome_password_blob(&blob, master_key) {
                        Ok(pw) if !pw.is_empty() => out.push((url, user, pw)),
                        Err("v20") => skipped_v20 += 1,
                        _ => {}
                    }
                }
            }
        }
    }

    let _ = fs::remove_file(&tmp_db);
    (out, skipped_v20)
}

/// Imports standard Chrome exported passwords CSV (`name,url,username,password[,note]`).
pub fn import_chrome_passwords_csv(vault: &mut CredentialVault, csv_text: &str) -> usize {
    let mut imported = 0usize;
    for (idx, line) in csv_text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let cols = parse_csv_line(trimmed);
        if idx == 0 && cols.get(1).map(|s| s.eq_ignore_ascii_case("url")).unwrap_or(false) {
            continue;
        }
        if cols.len() >= 4 {
            let url = cols[1].trim();
            let user = cols[2].trim();
            let pass = cols[3].trim();
            if !url.is_empty() && !user.is_empty() && !pass.is_empty() {
                if upsert_vault_entry(vault, url, user, pass, "csv") {
                    imported += 1;
                }
            }
        }
    }
    if imported > 0 {
        save_vault(vault);
    }
    imported
}

fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                if in_quotes && chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = !in_quotes;
                }
            }
            ',' if !in_quotes => {
                fields.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    fields.push(cur);
    fields
}

/// Syncs tabs, bookmarks, and logins from local Chrome profiles.
pub fn sync_from_chrome(
    vault: &mut CredentialVault,
    bookmarks: &mut Vec<Bookmark>,
    sync_tabs: bool,
    sync_logins: bool,
    sync_bookmarks: bool,
) -> (Vec<SyncedChromeTab>, ChromeSyncReport) {
    let Some(user_data) = chrome_user_data_dir() else {
        return (
            Vec::new(),
            ChromeSyncReport {
                chrome_found: false,
                note: "No local Google Chrome profile found.".into(),
                last_sync_at: Some(Utc::now()),
                ..Default::default()
            },
        );
    };

    let profiles = find_chrome_profiles(&user_data);
    let master_key = if sync_logins {
        read_chrome_master_key(&user_data)
    } else {
        None
    };

    let mut all_tabs = Vec::new();
    let mut seen_tab_urls = HashSet::new();
    let mut logins_synced = 0usize;
    let mut logins_skipped_v20 = 0usize;
    let mut bookmarks_synced = 0usize;

    for profile in &profiles {
        if sync_tabs {
            for t in read_chrome_open_tabs(profile) {
                if seen_tab_urls.insert(t.url.clone()) {
                    all_tabs.push(t);
                }
            }
        }

        if sync_bookmarks {
            for (title, url) in read_chrome_bookmarks(profile) {
                if !bookmarks.iter().any(|b| b.url == url) {
                    bookmarks.push(Bookmark {
                        id: format!("bm-chrome-{}", bookmarks.len() + 1),
                        title,
                        url,
                        created_at: Utc::now(),
                    });
                    bookmarks_synced += 1;
                }
            }
        }

        if sync_logins {
            let (logins, v20) = read_chrome_logins(profile, master_key.as_deref());
            logins_skipped_v20 += v20;
            for (url, user, pw) in logins {
                if upsert_vault_entry(vault, &url, &user, &pw, "chrome") {
                    logins_synced += 1;
                }
            }
        }
    }

    if logins_synced > 0 {
        save_vault(vault);
    }

    let note = if logins_skipped_v20 > 0 {
        format!(
            "Chrome v20 app-bound encryption protected {logins_skipped_v20} login(s); export CSV from Chrome Password Manager to import those."
        )
    } else {
        "Synced cleanly from local Chrome profile.".into()
    };

    let report = ChromeSyncReport {
        chrome_found: true,
        profiles_scanned: profiles.len(),
        tabs_found: all_tabs.len(),
        tabs_opened: 0,
        logins_synced,
        logins_skipped_v20,
        bookmarks_synced,
        last_sync_at: Some(Utc::now()),
        note,
    };

    (all_tabs, report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_snss_tab_navigation_records() {
        let mut snss = Vec::new();
        snss.extend_from_slice(b"SNSS");
        snss.extend_from_slice(&1u32.to_le_bytes());

        // Build UpdateTabNavigation (cmd_id = 1)
        let url = b"https://github.com/ahpah-dev/Arthos";
        let title_u16: Vec<u16> = "Arthos Repo".encode_utf16().collect();
        let mut pickle = Vec::new();
        pickle.extend_from_slice(&42i32.to_le_bytes()); // tab_id
        pickle.extend_from_slice(&0i32.to_le_bytes()); // nav_idx
        pickle.extend_from_slice(&(url.len() as u32).to_le_bytes());
        pickle.extend_from_slice(url);
        while pickle.len() % 4 != 0 {
            pickle.push(0);
        }
        pickle.extend_from_slice(&(title_u16.len() as u32).to_le_bytes());
        for ch in title_u16 {
            pickle.extend_from_slice(&ch.to_le_bytes());
        }
        while pickle.len() % 4 != 0 {
            pickle.push(0);
        }

        let mut payload = Vec::new();
        payload.extend_from_slice(&(pickle.len() as u32).to_le_bytes());
        payload.extend_from_slice(&pickle);

        let rec_size = (1 + payload.len()) as u16;
        snss.extend_from_slice(&rec_size.to_le_bytes());
        snss.push(1u8);
        snss.extend_from_slice(&payload);

        let tabs = parse_snss_tabs(&snss);
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0].url, "https://github.com/ahpah-dev/Arthos");
        assert_eq!(tabs[0].title, "Arthos Repo");
    }

    #[test]
    fn imports_chrome_passwords_csv_and_matches_origin() {
        let mut vault = CredentialVault::default();
        let csv = "name,url,username,password,note\nGitHub,https://github.com/login,david,s3cr3t!,\n";
        let added = import_chrome_passwords_csv(&mut vault, csv);
        assert_eq!(added, 1);

        let matches = matching_logins_for_url(&vault, "https://github.com/session");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, "david");
        assert_eq!(matches[0].1, "s3cr3t!");
    }
}
