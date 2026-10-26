use std::ffi::c_void;

use native_windows_gui as nwg;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE};
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, GetSysColor, InvalidateRect, COLOR_HIGHLIGHT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, GetWindowLongW, SendMessageW, SetClassLongPtrW,
    SetForegroundWindow, SetWindowLongW, SetWindowPos, GCLP_HBRBACKGROUND, GWL_STYLE, HWND_BOTTOM,
    HWND_TOP, SM_CXSCREEN, SM_CYSCREEN, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, WS_CLIPSIBLINGS, WS_MAXIMIZEBOX, WS_THICKFRAME,
};

pub const BG_RGB: [u8; 3] = [255, 255, 255];

pub fn accent_rgb() -> [u8; 3] {
    let color = unsafe { GetSysColor(COLOR_HIGHLIGHT) };
    [
        (color & 0xFF) as u8,
        ((color >> 8) & 0xFF) as u8,
        ((color >> 16) & 0xFF) as u8,
    ]
}

pub fn center(width: i32, height: i32) -> (i32, i32) {
    unsafe {
        (
            (GetSystemMetrics(SM_CXSCREEN) - width) / 2,
            (GetSystemMetrics(SM_CYSCREEN) - height) / 2,
        )
    }
}

pub fn hwnd(window: &nwg::Window) -> Option<HWND> {
    window.handle.hwnd().map(|h| HWND(h as *mut c_void))
}

pub fn send_to_back(handle: &nwg::ControlHandle) {
    if let Some(hwnd) = handle.hwnd() {
        unsafe {
            let _ = SetWindowPos(
                HWND(hwnd as *mut c_void),
                Some(HWND_BOTTOM),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
}

pub fn set_window_background(window: &nwg::Window, rgb: [u8; 3]) {
    let Some(hwnd) = hwnd(window) else {
        return;
    };
    unsafe {
        let color = (rgb[0] as u32) | ((rgb[1] as u32) << 8) | ((rgb[2] as u32) << 16);
        let brush = CreateSolidBrush(COLORREF(color));
        if brush.is_invalid() {
            return;
        }
        SetClassLongPtrW(hwnd, GCLP_HBRBACKGROUND, brush.0 as isize);
        let _ = InvalidateRect(Some(hwnd), None, true);
    }
}

pub fn set_clipsiblings(handle: &nwg::ControlHandle) {
    if let Some(hwnd) = handle.hwnd() {
        unsafe {
            let hwnd = HWND(hwnd as *mut c_void);
            let style = GetWindowLongW(hwnd, GWL_STYLE);
            SetWindowLongW(hwnd, GWL_STYLE, style | WS_CLIPSIBLINGS.0 as i32);
        }
    }
}

pub fn control_to_top(handle: &nwg::ControlHandle) {
    if let Some(hwnd) = handle.hwnd() {
        unsafe {
            let _ = SetWindowPos(
                HWND(hwnd as *mut c_void),
                Some(HWND_TOP),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
}

pub fn bring_to_front(window: &nwg::Window) {
    if let Some(hwnd) = hwnd(window) {
        unsafe {
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

pub fn apply_chrome(window: &nwg::Window) {
    let Some(hwnd) = hwnd(window) else {
        return;
    };
    unsafe {
        let preference: i32 = 2;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const _ as *const c_void,
            4,
        );

        let style = GetWindowLongW(hwnd, GWL_STYLE);
        let remove = (WS_THICKFRAME.0 | WS_MAXIMIZEBOX.0) as i32;
        SetWindowLongW(hwnd, GWL_STYLE, style & !remove);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED,
        );
    }
}

pub fn marquee(progress: &nwg::ProgressBar, running: bool) {
    let Some(hwnd) = progress.handle.hwnd().map(|h| HWND(h as *mut c_void)) else {
        return;
    };
    unsafe {
        let style = GetWindowLongW(hwnd, GWL_STYLE);
        let style = if running {
            style | 0x0000_0008
        } else {
            style & !0x0000_0008
        };
        SetWindowLongW(hwnd, GWL_STYLE, style);
        SendMessageW(
            hwnd,
            0x040A,
            Some(WPARAM(if running { 1 } else { 0 })),
            Some(LPARAM(30)),
        );
    }
}
