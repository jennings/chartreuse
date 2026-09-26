//! Windows: on-screen window enumeration for window selection.
//!
//! `EnumWindows` walks the top-level windows front to back. The ones window
//! selection offers, and how their bounds are measured, are described in
//! `window_filter.rs`: visible, uncloaked, not minimized, not tool windows, not
//! the shell, not Chartreuse's own, with DWM's extended frame bounds converted
//! to logical coordinates through the monitor layout.
//!
//! A window's [`WindowId`](chartreuse_core::window::WindowId) is its `HWND`.

use std::collections::HashMap;

use ::windows::core::{BOOL, PWSTR};
use ::windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use ::windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use ::windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use ::windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowLongW, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, GWL_EXSTYLE,
};
use chartreuse_core::geometry::PhysicalRect;
use chartreuse_core::window::WindowInfo;
use chartreuse_core::Result;
use futures::future::{self, BoxFuture, FutureExt};

use super::displays::{enumerate, physical_rect};
use super::util::{from_wide, platform_error};
use super::window_filter::{describe, is_selectable, owner_name, Candidate, Listed};
use crate::window_list::WindowList;

/// The Windows [`WindowList`] backend.
#[derive(Debug, Default)]
pub struct WindowsWindowList;

impl WindowsWindowList {
    pub fn new() -> Self {
        Self
    }
}

impl WindowList for WindowsWindowList {
    fn windows(&self) -> BoxFuture<'static, Result<Vec<WindowInfo>>> {
        // Enumeration only reads window-manager state and takes a few
        // milliseconds, so it runs right here.
        future::ready(list_windows()).boxed()
    }
}

fn list_windows() -> Result<Vec<WindowInfo>> {
    let layout = enumerate()?.layout;
    // SAFETY: no preconditions.
    let own_pid = unsafe { GetCurrentProcessId() };
    let mut owners: HashMap<u32, String> = HashMap::new();
    let listed = top_level_windows()?
        .into_iter()
        .filter_map(|hwnd| {
            let candidate = candidate(hwnd);
            if !is_selectable(&candidate, own_pid) {
                return None;
            }
            let pid = candidate.pid;
            Some(Listed {
                hwnd: hwnd.0 as usize as u64,
                pid,
                title: title(hwnd),
                owner_name: owners
                    .entry(pid)
                    .or_insert_with(|| process_name(pid))
                    .clone(),
                bounds: frame_bounds(hwnd)?,
            })
        })
        .collect();
    Ok(describe(listed, &layout))
}

/// Every top-level window, front to back.
fn top_level_windows() -> Result<Vec<HWND>> {
    let mut windows: Vec<HWND> = Vec::new();
    // SAFETY: `collect_window` only runs during this call and receives a pointer
    // to `windows`, which outlives it.
    unsafe {
        EnumWindows(
            Some(collect_window),
            LPARAM(std::ptr::from_mut(&mut windows) as isize),
        )
    }
    .map_err(|e| platform_error("EnumWindows", &e))?;
    Ok(windows)
}

/// `WNDENUMPROC`: appends the window to the `Vec<HWND>` behind `data`.
unsafe extern "system" fn collect_window(hwnd: HWND, data: LPARAM) -> BOOL {
    // SAFETY: `top_level_windows` passes a pointer to a live `Vec<HWND>`, used by
    // nothing else during the enumeration.
    let windows = unsafe { &mut *(data.0 as *mut Vec<HWND>) };
    windows.push(hwnd);
    true.into()
}

/// The attributes [`is_selectable`] needs.
fn candidate(hwnd: HWND) -> Candidate {
    let mut pid = 0;
    let mut cloaked: u32 = 0;
    let mut class = [0u16; 64];
    // SAFETY: `hwnd` came from EnumWindows (a stale handle only makes the calls
    // fail), and every out-pointer and buffer is valid for its stated size.
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let cloaked = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            std::ptr::from_mut(&mut cloaked).cast(),
            size_of::<u32>() as u32,
        )
        .is_ok()
            && cloaked != 0;
        let class_len = usize::try_from(GetClassNameW(hwnd, &mut class)).unwrap_or(0);
        Candidate {
            pid,
            visible: IsWindowVisible(hwnd).as_bool(),
            minimized: IsIconic(hwnd).as_bool(),
            cloaked,
            ex_style: GetWindowLongW(hwnd, GWL_EXSTYLE) as u32,
            class: from_wide(&class[..class_len]),
        }
    }
}

/// The window's title, or an empty string.
fn title(hwnd: HWND) -> String {
    // SAFETY: `hwnd` came from EnumWindows; the buffer is as long as stated.
    unsafe {
        let Ok(length) = usize::try_from(GetWindowTextLengthW(hwnd)) else {
            return String::new();
        };
        let mut buffer = vec![0u16; length + 1];
        let copied = usize::try_from(GetWindowTextW(hwnd, &mut buffer)).unwrap_or(0);
        String::from_utf16_lossy(&buffer[..copied.min(length)])
    }
}

/// The window's visible frame: DWM's extended frame bounds, which leave out the
/// invisible resize borders and the shadow, or the window rectangle if DWM has
/// none. `None` if neither can be read.
pub(super) fn frame_bounds(hwnd: HWND) -> Option<PhysicalRect> {
    let mut rect = RECT::default();
    // SAFETY: `rect` is a writable RECT of the stated size.
    let dwm = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            std::ptr::from_mut(&mut rect).cast(),
            size_of::<RECT>() as u32,
        )
    };
    // SAFETY: as above.
    if dwm.is_err() && unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
        return None;
    }
    Some(physical_rect(rect))
}

/// The name of process `pid`'s executable without its extension, or an empty
/// string if the process cannot be queried.
fn process_name(pid: u32) -> String {
    // SAFETY: the handle is closed below; the buffer is as long as stated.
    unsafe {
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return String::new();
        };
        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        let queried = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        );
        let _ = CloseHandle(process);
        match queried {
            Ok(()) => owner_name(&String::from_utf16_lossy(&buffer[..length as usize])),
            Err(_) => String::new(),
        }
    }
}
