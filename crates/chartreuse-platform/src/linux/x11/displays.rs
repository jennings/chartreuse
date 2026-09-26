//! X11: display enumeration through RandR 1.5 monitors, falling back to the
//! whole root window as one display on servers without it.
//!
//! See [`crate::linux::logic::randr`] for the logical space: the root
//! window's pixels divided by the one scale factor winit uses, found the way
//! winit finds it (`WINIT_X11_SCALE_FACTOR`, XSETTINGS `Xft/DPI`, the
//! `Xft.dpi` resource, the primary monitor's pixel density).

use chartreuse_core::display::DisplayInfo;
use chartreuse_core::geometry::{PhysicalRect, ScaleFactor};
use chartreuse_core::Result;
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xproto::ConnectionExt as _;

use super::connection::{self, failed, X11};
use crate::displays::Displays;
use crate::linux::logic::randr::{self, Monitor, ScaleSources};
use crate::linux::logic::xsettings;

/// The X11 [`Displays`] backend.
#[derive(Debug, Default)]
pub struct X11Displays;

impl X11Displays {
    pub fn new() -> Self {
        Self
    }
}

impl Displays for X11Displays {
    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        let desktop = Desktop::query(connection::get()?)?;
        Ok(desktop.displays().map(|(_, display)| display).collect())
    }
}

/// The monitors and the scale of the logical space, for the backends that
/// convert between root-window pixels and logical coordinates.
#[derive(Debug, Clone)]
pub struct Desktop {
    pub monitors: Vec<Monitor>,
    pub scale: ScaleFactor,
}

impl Desktop {
    pub fn query(x11: &X11) -> Result<Self> {
        let monitors = monitors(x11)?;
        let env_override = std::env::var("WINIT_X11_SCALE_FACTOR").ok();
        let sources = ScaleSources {
            env_override: env_override.as_deref().filter(|value| !value.is_empty()),
            xsettings_dpi: xsettings_dpi(x11),
            resource_dpi: resource_dpi(x11),
        };
        let scale = randr::desktop_scale(sources, &monitors);
        Ok(Self { monitors, scale })
    }

    /// Every monitor's root-window area with its display.
    pub fn displays(&self) -> impl Iterator<Item = (PhysicalRect, DisplayInfo)> + '_ {
        let displays = randr::displays(&self.monitors, self.scale);
        self.monitors
            .iter()
            .map(|monitor| monitor.bounds)
            .zip(displays)
    }
}

fn monitors(x11: &X11) -> Result<Vec<Monitor>> {
    let root = x11.root();
    // GetMonitors is RandR 1.5; the server needs to hear which version the
    // client speaks first.
    let reply = x11
        .conn
        .randr_query_version(1, 5)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .filter(|version| (version.major_version, version.minor_version) >= (1, 5))
        .and_then(|_| x11.conn.randr_get_monitors(root, true).ok())
        .and_then(|cookie| cookie.reply().ok());
    let Some(reply) = reply.filter(|reply| !reply.monitors.is_empty()) else {
        tracing::debug!("RandR 1.5 is unavailable; treating the root window as one display");
        let screen = x11.screen();
        return Ok(vec![Monitor {
            id: u64::from(root),
            name: "Screen".into(),
            bounds: PhysicalRect::new(
                0,
                0,
                u32::from(screen.width_in_pixels),
                u32::from(screen.height_in_pixels),
            ),
            millimeters: (
                u32::from(screen.width_in_millimeters),
                u32::from(screen.height_in_millimeters),
            ),
            primary: true,
        }]);
    };
    reply
        .monitors
        .iter()
        .map(|monitor| {
            let name = x11
                .conn
                .get_atom_name(monitor.name)
                .map_err(|e| failed("naming a monitor", e))?
                .reply()
                .map_err(|e| failed("naming a monitor", e))?
                .name;
            Ok(Monitor {
                id: u64::from(monitor.outputs.first().copied().unwrap_or(monitor.name)),
                name: String::from_utf8_lossy(&name).into_owned(),
                bounds: PhysicalRect::new(
                    i32::from(monitor.x),
                    i32::from(monitor.y),
                    u32::from(monitor.width),
                    u32::from(monitor.height),
                ),
                millimeters: (monitor.width_in_millimeters, monitor.height_in_millimeters),
                primary: monitor.primary,
            })
        })
        .collect()
}

/// `Xft/DPI` from the XSETTINGS manager of the default screen, if one runs.
fn xsettings_dpi(x11: &X11) -> Option<f64> {
    let selection = x11.atom(&format!("_XSETTINGS_S{}", x11.screen)).ok()?;
    let owner = x11
        .conn
        .get_selection_owner(selection)
        .ok()?
        .reply()
        .ok()?
        .owner;
    if owner == x11rb::NONE {
        return None;
    }
    let settings = x11.atoms._XSETTINGS_SETTINGS;
    let block = x11.property8(owner, settings, settings).ok()?;
    xsettings::xft_dpi(&block)
}

/// `Xft.dpi` from the X resource database (`xrdb`).
fn resource_dpi(x11: &X11) -> Option<f64> {
    let database = x11rb::resource_manager::new_from_default(&x11.conn).ok()?;
    database
        .get_string("Xft.dpi", "")
        .and_then(|dpi| dpi.trim().parse::<f64>().ok())
        .filter(|dpi| *dpi > 0.0)
}
