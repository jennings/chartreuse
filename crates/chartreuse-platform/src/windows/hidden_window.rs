//! Windows: invisible windows that receive messages for the backends (hotkeys,
//! the tray icon, clipboard ownership).
//!
//! A [`HiddenWindow`] is created on the calling thread — the main thread, whose
//! message loop winit runs — so its window procedure is called from that loop.
//! It owns a [`Handler`], reachable from the window procedure through
//! `GWLP_USERDATA`, and destroys the window when dropped.

use std::ffi::c_void;
use std::marker::PhantomData;

use ::windows::core::PCWSTR;
use ::windows::Win32::Foundation::{
    GetLastError, ERROR_CLASS_ALREADY_EXISTS, HWND, LPARAM, LRESULT, WPARAM,
};
use ::windows::Win32::System::LibraryLoader::GetModuleHandleW;
use ::windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowLongPtrW, RegisterClassExW,
    SetWindowLongPtrW, CREATESTRUCTW, GWLP_USERDATA, HWND_MESSAGE, WINDOW_EX_STYLE, WM_NCCREATE,
    WNDCLASSEXW, WS_OVERLAPPED,
};
use chartreuse_core::Result;

use super::util::{platform_error, wide};

/// Handles a [`HiddenWindow`]'s messages.
pub(super) trait Handler: 'static {
    /// The window class name, unique per handler type.
    const CLASS: &'static str;

    /// Handles `message`, or returns `None` to leave it to `DefWindowProcW`.
    fn handle(&self, hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT>;
}

/// An invisible window owning its [`Handler`]. Dropping it destroys the window.
pub(super) struct HiddenWindow<H: Handler> {
    hwnd: HWND,
    handler: *mut H,
    // Window procedures run on the creating thread only.
    _not_send: PhantomData<*mut H>,
}

impl<H: Handler> std::fmt::Debug for HiddenWindow<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HiddenWindow")
            .field("class", &H::CLASS)
            .field("hwnd", &self.hwnd)
            .finish_non_exhaustive()
    }
}

impl<H: Handler> HiddenWindow<H> {
    /// Creates a message-only window (`HWND_MESSAGE`), which receives posted and
    /// sent messages but no broadcasts.
    pub(super) fn new(handler: H) -> Result<Self> {
        let class = wide(H::CLASS);
        // SAFETY: a null module name means this executable.
        let instance = unsafe { GetModuleHandleW(None) }
            .map_err(|e| platform_error("GetModuleHandleW", &e))?;
        let class_info = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(window_proc::<H>),
            hInstance: instance.into(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        // SAFETY: `class_info` and the class name it points to are valid for the
        // call; GetLastError has no preconditions. A class registered by an earlier
        // window of this type is reused.
        let registered = unsafe {
            RegisterClassExW(&class_info) != 0 || GetLastError() == ERROR_CLASS_ALREADY_EXISTS
        };
        if !registered {
            return Err(platform_error(
                "RegisterClassExW",
                &::windows::core::Error::from_thread(),
            ));
        }

        let handler = Box::into_raw(Box::new(handler));
        // SAFETY: the class is registered; `handler` stays valid until the window
        // is destroyed (see `Drop`), and WM_NCCREATE stores it for `window_proc`.
        let created = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class.as_ptr()),
                PCWSTR(class.as_ptr()),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(instance.into()),
                Some(handler.cast::<c_void>().cast_const()),
            )
        };
        match created {
            Ok(hwnd) => Ok(Self {
                hwnd,
                handler,
                _not_send: PhantomData,
            }),
            Err(error) => {
                // SAFETY: the window does not exist, so nothing else holds it.
                drop(unsafe { Box::from_raw(handler) });
                Err(platform_error("CreateWindowExW", &error))
            }
        }
    }

    pub(super) fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

impl<H: Handler> Drop for HiddenWindow<H> {
    fn drop(&mut self) {
        // SAFETY: the window belongs to this thread (`HiddenWindow` is not Send).
        // The window procedure stops seeing the handler before it is freed, even
        // if the window outlived a failed DestroyWindow.
        unsafe {
            set_user_data(self.hwnd, 0);
            if let Err(error) = DestroyWindow(self.hwnd) {
                tracing::warn!(%error, class = H::CLASS, "DestroyWindow failed");
            }
            drop(Box::from_raw(self.handler));
        }
    }
}

/// The window procedure of `H`'s class.
unsafe extern "system" fn window_proc<H: Handler>(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: WM_NCCREATE's lparam is the CREATESTRUCTW whose lpCreateParams is
    // the handler pointer from `HiddenWindow::new`; later messages read it back
    // from GWLP_USERDATA, which `Drop` clears before freeing the handler.
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lparam.0 as *const CREATESTRUCTW);
            set_user_data(hwnd, create.lpCreateParams as isize);
        } else if let Some(handler) = (user_data(hwnd) as *const H).as_ref()
            && let Some(result) = handler.handle(hwnd, message, wparam, lparam)
        {
            return result;
        }
        DefWindowProcW(hwnd, message, wparam, lparam)
    }
}

// `GWLP_USERDATA` through the pointer-sized accessors, which the `windows` crate
// provides on 64-bit targets (the ones Chartreuse builds for).

unsafe fn set_user_data(hwnd: HWND, value: isize) {
    // SAFETY: the caller passes a window of this thread.
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, value) };
}

unsafe fn user_data(hwnd: HWND) -> isize {
    // SAFETY: the caller passes a window of this thread.
    unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) }
}
