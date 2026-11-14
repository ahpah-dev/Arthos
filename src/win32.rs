#[cfg(target_os = "windows")]
pub fn polish_window(window: &tao::window::Window, dark: bool) {
    use std::ffi::c_void;
    use tao::rwh_06::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Graphics::Dwm::{
        DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DwmSetWindowAttribute,
    };

    if let Ok(handle) = window.window_handle() {
        if let RawWindowHandle::Win32(win32) = handle.as_raw() {
            let hwnd = win32.hwnd.get() as *mut c_void;
            let dark_val: i32 = if dark { 1 } else { 0 };
            let corner_pref: u32 = 2; // DWMWCP_ROUND
            unsafe {
                let _ = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
                    &dark_val as *const i32 as *const c_void,
                    std::mem::size_of::<i32>() as u32,
                );
                let _ = DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                    &corner_pref as *const u32 as *const c_void,
                    std::mem::size_of::<u32>() as u32,
                );
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub fn top_child_hwnd(window: &tao::window::Window) -> isize {
    use std::ffi::c_void;
    use tao::rwh_06::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GW_CHILD, GetWindow};

    if let Ok(handle) = window.window_handle() {
        if let RawWindowHandle::Win32(win32) = handle.as_raw() {
            let parent = win32.hwnd.get() as *mut c_void;
            let child = unsafe { GetWindow(parent, GW_CHILD) };
            return child as isize;
        }
    }
    0
}

#[cfg(target_os = "windows")]
pub fn bring_hwnd_to_top(hwnd_val: isize) {
    use std::ffi::c_void;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SetWindowPos,
    };

    if hwnd_val != 0 {
        unsafe {
            let _ = SetWindowPos(
                hwnd_val as *mut c_void,
                HWND_TOP,
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOOWNERZORDER | SWP_NOSIZE,
            );
        }
    }
}

#[cfg(target_os = "windows")]
pub fn apply_webview_theme(webview: &wry::WebView, dark: bool) {
    use wry::{Theme, WebViewExtWindows};
    let _ = webview.set_theme(if dark { Theme::Dark } else { Theme::Light });
}

#[cfg(not(target_os = "windows"))]
pub fn polish_window(_window: &tao::window::Window, _dark: bool) {}

#[cfg(not(target_os = "windows"))]
pub fn top_child_hwnd(_window: &tao::window::Window) -> isize {
    0
}

#[cfg(not(target_os = "windows"))]
pub fn bring_hwnd_to_top(_hwnd_val: isize) {}

#[cfg(not(target_os = "windows"))]
pub fn apply_webview_theme(_webview: &wry::WebView, _dark: bool) {}
