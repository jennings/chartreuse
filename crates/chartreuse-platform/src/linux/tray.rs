//! The tray icon of both Linux backends: a StatusNotifierItem over D-Bus, which
//! KDE Plasma, Xfce, Cinnamon, MATE, LXQt, Budgie, and the wlroots panels
//! (Waybar, …) show natively and GNOME shows with the AppIndicator extension.
//!
//! [`install`] serves the item and its com.canonical.dbusmenu menu from a
//! `ksni` service thread and registers it with the StatusNotifierWatcher. The
//! panel calls back into that thread when the user picks a menu item, and the
//! thread forwards the [`MenuAction`] through an [`EventSender`]. Dropping the
//! registration shuts the service down, which removes the icon.

use std::path::Path;

use chartreuse_core::flavor::{self, Flavor};
use chartreuse_core::{Error, Result};
use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::StandardItem;
use ksni::{Category, Icon, MenuItem, ToolTip, Tray};

use super::logic::tray::{argb32, menu_label};
use crate::event::{self, EventSender, Registration};
use crate::status_item::{MenuAction, MenuEntry, StatusItemHandle, MENU};

/// The tray icon in every size `cargo xtask icons` renders, for the current
/// flavor. Panels pick the size closest to theirs.
const ICONS: [&[u8]; 6] = match Flavor::CURRENT {
    Flavor::Release => [
        include_bytes!("../../../../assets/icon/generated/tray-release-16.png"),
        include_bytes!("../../../../assets/icon/generated/tray-release-22.png"),
        include_bytes!("../../../../assets/icon/generated/tray-release-24.png"),
        include_bytes!("../../../../assets/icon/generated/tray-release-32.png"),
        include_bytes!("../../../../assets/icon/generated/tray-release-48.png"),
        include_bytes!("../../../../assets/icon/generated/tray-release-64.png"),
    ],
    Flavor::Development => [
        include_bytes!("../../../../assets/icon/generated/tray-development-16.png"),
        include_bytes!("../../../../assets/icon/generated/tray-development-22.png"),
        include_bytes!("../../../../assets/icon/generated/tray-development-24.png"),
        include_bytes!("../../../../assets/icon/generated/tray-development-32.png"),
        include_bytes!("../../../../assets/icon/generated/tray-development-48.png"),
        include_bytes!("../../../../assets/icon/generated/tray-development-64.png"),
    ],
};

/// Shows the tray icon with the [`MENU`].
///
/// # Errors
///
/// [`Error::Platform`] when there is no session bus, the desktop has no
/// StatusNotifierWatcher (GNOME without the AppIndicator extension), or no
/// panel shows StatusNotifierItems.
pub fn install() -> Result<StatusItemHandle> {
    let icons = ICONS.iter().map(|png| icon(png)).collect::<Result<_>>()?;
    let (actions, receiver) = event::channel();
    let tray = ChartreuseTray { actions, icons };
    // Flatpak's D-Bus proxy lets apps own only names under their app id, not
    // the `StatusNotifierItem-<pid>-<n>` name the specification asks for;
    // panels accept an item registered by its unique connection name.
    let sandboxed = Path::new("/.flatpak-info").exists();
    let handle = tray
        .disable_dbus_name(sandboxed)
        .spawn()
        .map_err(|error| Error::Platform(describe(&error)))?;
    Ok(StatusItemHandle {
        actions: receiver,
        registration: Registration::new(Service(handle)),
    })
}

/// Decodes one of the [`ICONS`].
fn icon(png: &[u8]) -> Result<Icon> {
    let image = chartreuse_imaging::decode(png)?;
    let side = |length: u32| {
        i32::try_from(length).map_err(|_| Error::InvalidImage("tray icon too large".into()))
    };
    Ok(Icon {
        width: side(image.width())?,
        height: side(image.height())?,
        data: argb32(&image),
    })
}

/// What went wrong, in terms of what the user can fix.
fn describe(error: &ksni::Error) -> String {
    match error {
        ksni::Error::Watcher(_) => format!(
            "the desktop shows no StatusNotifierItem tray icons ({error}); on GNOME, \
             install the AppIndicator extension"
        ),
        ksni::Error::WontShow => {
            format!("no panel is showing StatusNotifierItem tray icons ({error})")
        }
        _ => format!("the tray icon could not be installed: {error}"),
    }
}

/// The item's state, owned by the service thread.
struct ChartreuseTray {
    actions: EventSender<MenuAction>,
    icons: Vec<Icon>,
}

impl Tray for ChartreuseTray {
    fn id(&self) -> String {
        flavor::BUNDLE_ID.into()
    }

    fn title(&self) -> String {
        flavor::DISPLAY_NAME.into()
    }

    fn category(&self) -> Category {
        Category::ApplicationStatus
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icons.clone()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: flavor::DISPLAY_NAME.into(),
            ..ToolTip::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        MENU.iter()
            .map(|entry| match *entry {
                MenuEntry::Separator => MenuItem::Separator,
                MenuEntry::Action(action) => StandardItem {
                    label: menu_label(&action.label()),
                    activate: Box::new(move |tray: &mut Self| {
                        tray.actions.send(action);
                    }),
                    ..StandardItem::default()
                }
                .into(),
            })
            .collect()
    }

    fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
        // Keep serving: the item registers again when a watcher (a restarted
        // panel) comes back.
        tracing::warn!("the StatusNotifierWatcher went away: {reason:?}");
        true
    }
}

/// Keeps the service running; dropping it removes the icon.
struct Service(Handle<ChartreuseTray>);

impl Drop for Service {
    fn drop(&mut self) {
        // Don't wait for the service thread: the icon goes away as soon as it
        // handles the request.
        drop(self.0.shutdown());
    }
}
