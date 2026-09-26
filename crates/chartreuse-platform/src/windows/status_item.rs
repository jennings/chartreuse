//! Windows: the status item, a notification-area (tray) icon.
//!
//! [`WindowsStatusItem::install`] creates a hidden top-level window on the main
//! thread and adds a `Shell_NotifyIcon` icon (`NOTIFYICON_VERSION_4`) that reports
//! to it. Clicking the icon, right-clicking it, or selecting it with the keyboard
//! shows the shared [`MENU`](crate::MENU) as a popup menu at the icon, like the
//! macOS menu bar item; the chosen [`MenuAction`](crate::MenuAction) goes through
//! the [`EventSender`]. The window is never shown and is a tool window, so
//! Chartreuse gets no taskbar button from it.
//!
//! When Explorer restarts it broadcasts `TaskbarCreated`, and the icon is added
//! again. The icon is the flavor's `assets/icon/generated/tray-*.ico`, at the small
//! icon size for the system DPI. Dropping the [`Registration`] removes the icon.

use ::windows::core::{w, PCWSTR};
use ::windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use ::windows::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
use ::windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE,
    NIM_SETVERSION, NIN_SELECT, NOTIFYICONDATAW, NOTIFYICON_VERSION_4,
};
use ::windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, DestroyIcon, DestroyMenu, PostMessageW,
    RegisterWindowMessageW, SetForegroundWindow, TrackPopupMenu, HICON, HMENU, LR_DEFAULTCOLOR,
    MF_SEPARATOR, MF_STRING, SM_CXSMICON, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP,
    WM_CONTEXTMENU, WM_NULL,
};
use chartreuse_core::{flavor, Error, Result};

use super::hidden_window::{Handler, HiddenWindow, Kind};
use super::ico::{best_entry, entries, tray_icon};
use super::tray_menu::{action_for_command, anchor, command_id, menu_text, notification};
use super::util::{copy_wide, platform_error, wide};
use crate::event::{self, EventSender, Registration};
use crate::status_item::{MenuAction, MenuEntry, StatusItem, StatusItemHandle, MENU};

/// The message the icon sends to the window.
const CALLBACK_MESSAGE: u32 = WM_APP + 1;
/// The icon's id within the window.
const ICON_ID: u32 = 1;
/// `NIN_KEYSELECT`: the icon was selected with the keyboard.
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;

/// The Windows [`StatusItem`] backend.
#[derive(Debug, Default)]
pub struct WindowsStatusItem;

impl WindowsStatusItem {
    pub fn new() -> Self {
        Self
    }
}

impl StatusItem for WindowsStatusItem {
    fn install(&self) -> Result<StatusItemHandle> {
        let (sender, actions) = event::channel();
        let icon = Icon::load()?;
        let menu = Menu::build()?;
        // SAFETY: the name is a valid NUL-terminated string.
        let taskbar_created = unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) };
        let window = HiddenWindow::new(
            Kind::TopLevel,
            Tray {
                sender,
                menu,
                icon,
                taskbar_created,
            },
        )?;
        // Built after the window exists, so dropping it on failure removes nothing.
        let installed = Installed(window);
        if !installed.0.handler().add(installed.0.hwnd()) {
            return Err(Error::Platform(
                "the notification area refused the tray icon".into(),
            ));
        }
        tracing::info!("tray icon installed");
        Ok(StatusItemHandle {
            actions,
            registration: Registration::new(installed),
        })
    }
}

/// An installed tray icon. Dropping it removes the icon, then destroys the window
/// (and with it the menu and the icon image).
struct Installed(HiddenWindow<Tray>);

impl Drop for Installed {
    fn drop(&mut self) {
        let data = self.0.handler().notify_data(self.0.hwnd());
        // SAFETY: `data` identifies this window's icon.
        if unsafe { Shell_NotifyIconW(NIM_DELETE, &data) }.as_bool() {
            tracing::info!("tray icon removed");
        }
    }
}

/// The tray window's state and message handling.
struct Tray {
    sender: EventSender<MenuAction>,
    menu: Menu,
    icon: Icon,
    /// The `TaskbarCreated` message id, or 0 if it could not be registered.
    taskbar_created: u32,
}

impl Tray {
    fn notify_data(&self, hwnd: HWND) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: ICON_ID,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: CALLBACK_MESSAGE,
            hIcon: self.icon.0,
            ..Default::default()
        };
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        copy_wide(&mut data.szTip, flavor::DISPLAY_NAME);
        data
    }

    /// Adds the icon to the notification area. Returns whether it was added.
    fn add(&self, hwnd: HWND) -> bool {
        let data = self.notify_data(hwnd);
        // SAFETY: `data` is fully initialized and describes this window's icon.
        unsafe {
            Shell_NotifyIconW(NIM_ADD, &data).as_bool()
                && Shell_NotifyIconW(NIM_SETVERSION, &data).as_bool()
        }
    }

    /// Shows the menu at `(x, y)` and sends the chosen action.
    fn show_menu(&self, hwnd: HWND, x: i32, y: i32) {
        // SAFETY: the menu and window are live. The foreground window and the
        // trailing WM_NULL make the menu close when the user clicks elsewhere
        // (KB135788).
        let command = unsafe {
            let _ = SetForegroundWindow(hwnd);
            let command = TrackPopupMenu(
                self.menu.0,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                x,
                y,
                None,
                hwnd,
                None,
            );
            let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
            command
        };
        let Some(action) = usize::try_from(command.0).ok().and_then(action_for_command) else {
            return;
        };
        tracing::debug!(?action, "tray menu choice");
        if !self.sender.send(action) {
            tracing::debug!(?action, "nothing is listening for tray menu choices");
        }
    }
}

impl Handler for Tray {
    const CLASS: &'static str = "ChartreuseTray";

    fn handle(&self, hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        if message == CALLBACK_MESSAGE {
            if let WM_CONTEXTMENU | NIN_SELECT | NIN_KEYSELECT = notification(lparam.0) {
                let (x, y) = anchor(wparam.0);
                self.show_menu(hwnd, x, y);
            }
            return Some(LRESULT(0));
        }
        if self.taskbar_created != 0 && message == self.taskbar_created {
            if !self.add(hwnd) {
                tracing::warn!("could not add the tray icon again after Explorer restarted");
            }
            return Some(LRESULT(0));
        }
        None
    }
}

/// The popup menu, destroyed on drop.
struct Menu(HMENU);

impl Menu {
    /// Builds [`MENU`], each action item with its [`command_id`].
    fn build() -> Result<Self> {
        // SAFETY: no preconditions.
        let menu =
            Self(unsafe { CreatePopupMenu() }.map_err(|e| platform_error("CreatePopupMenu", &e))?);
        for (index, entry) in MENU.iter().enumerate() {
            let (flags, id, label) = match entry {
                MenuEntry::Separator => (MF_SEPARATOR, 0, None),
                MenuEntry::Action(action) => (
                    MF_STRING,
                    command_id(index),
                    Some(wide(&menu_text(&action.label()))),
                ),
            };
            let text = label
                .as_ref()
                .map_or(PCWSTR::null(), |label| PCWSTR(label.as_ptr()));
            // SAFETY: the menu is live; `label` outlives the call.
            unsafe { AppendMenuW(menu.0, flags, id, text) }
                .map_err(|e| platform_error("AppendMenuW", &e))?;
        }
        Ok(menu)
    }
}

impl Drop for Menu {
    fn drop(&mut self) {
        // SAFETY: created by `build` and destroyed only here.
        let _ = unsafe { DestroyMenu(self.0) };
    }
}

/// The tray icon image, destroyed on drop.
struct Icon(HICON);

impl Icon {
    /// The flavor's tray icon at the small icon size for the system DPI.
    fn load() -> Result<Self> {
        // SAFETY: no preconditions.
        let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()) }.max(16);
        let entries = entries(tray_icon())
            .ok_or_else(|| Error::Platform("the built-in tray icon is malformed".into()))?;
        let entry = best_entry(&entries, size as u32)
            .ok_or_else(|| Error::Platform("the built-in tray icon is empty".into()))?;
        // SAFETY: `entry.bytes` is one complete icon image (a PNG).
        let icon = unsafe {
            CreateIconFromResourceEx(entry.bytes, true, 0x0003_0000, size, size, LR_DEFAULTCOLOR)
        }
        .map_err(|e| platform_error("CreateIconFromResourceEx", &e))?;
        Ok(Self(icon))
    }
}

impl Drop for Icon {
    fn drop(&mut self) {
        // SAFETY: created by `load` and destroyed only here.
        let _ = unsafe { DestroyIcon(self.0) };
    }
}
