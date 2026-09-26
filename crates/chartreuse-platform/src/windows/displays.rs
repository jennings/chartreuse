//! Windows: display enumeration through `EnumDisplayMonitors`.
//!
//! Each `HMONITOR` gives its rectangle in the physical virtual screen
//! (`GetMonitorInfoW`, which reports physical pixels because the app is Per-Monitor
//! DPI Aware v2 — see `crates/chartreuse/chartreuse.exe.manifest`) and its
//! effective DPI (`GetDpiForMonitor`). [`MonitorLayout`] turns those into logical
//! bounds. Display names come from the display configuration API (the monitor's
//! EDID name, or "Built-in Display" for a laptop panel).
//!
//! A display's [`DisplayId`] is its `HMONITOR`, which stays valid while the display
//! configuration is unchanged.

use std::collections::HashMap;

use ::windows::core::BOOL;
use ::windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_MODE_INFO,
    DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL,
    DISPLAYCONFIG_OUTPUT_TECHNOLOGY_LVDS, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED,
    DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME,
    QDC_ONLY_ACTIVE_PATHS,
};
use ::windows::Win32::Foundation::{ERROR_SUCCESS, LPARAM, RECT};
use ::windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use ::windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use ::windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;
use chartreuse_core::display::{DisplayId, DisplayInfo};
use chartreuse_core::geometry::{PhysicalRect, PhysicalSize};
use chartreuse_core::{Error, Result};

use super::layout::{scale_for_dpi, Monitor, MonitorLayout};
use super::util::{from_wide, platform_error};
use crate::displays::Displays;

/// The Windows [`Displays`] backend.
#[derive(Debug, Default)]
pub struct WindowsDisplays;

impl WindowsDisplays {
    pub fn new() -> Self {
        Self
    }
}

impl Displays for WindowsDisplays {
    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        Ok(enumerate()?.displays)
    }
}

/// The connected monitors: their layout and their descriptions, in the same
/// order.
#[derive(Debug)]
pub(super) struct Monitors {
    pub layout: MonitorLayout,
    pub displays: Vec<DisplayInfo>,
}

/// Enumerates the connected monitors. Works on any thread.
pub(super) fn enumerate() -> Result<Monitors> {
    let mut handles: Vec<HMONITOR> = Vec::new();
    // SAFETY: `collect_monitor` only runs during this call and receives a pointer to
    // `handles`, which outlives it.
    let ok = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect_monitor),
            LPARAM(std::ptr::from_mut(&mut handles) as isize),
        )
    };
    if !ok.as_bool() || handles.is_empty() {
        return Err(Error::Platform("no displays are connected".into()));
    }

    let names = friendly_names();
    let mut monitors = Vec::with_capacity(handles.len());
    let mut devices = Vec::with_capacity(handles.len());
    for &handle in &handles {
        let (monitor, device) = describe(handle)?;
        monitors.push(monitor);
        devices.push(device);
    }
    let layout = MonitorLayout::new(monitors.clone());
    let displays = handles
        .iter()
        .zip(&monitors)
        .zip(&devices)
        .enumerate()
        .map(|(index, ((handle, monitor), device))| DisplayInfo {
            id: DisplayId(handle.0 as usize as u64),
            name: names
                .get(device)
                .cloned()
                .unwrap_or_else(|| format!("Display {}", index + 1)),
            logical_bounds: layout.logical_bounds(index),
            pixel_size: monitor.physical.size,
            scale_factor: monitor.scale,
            is_primary: monitor.is_primary,
        })
        .collect();
    Ok(Monitors { layout, displays })
}

/// `MONITORENUMPROC`: appends the monitor to the `Vec<HMONITOR>` behind `data`.
unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    // SAFETY: `enumerate` passes a pointer to a live `Vec<HMONITOR>`, used by
    // nothing else during the enumeration.
    let handles = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
    handles.push(monitor);
    true.into()
}

/// The monitor's physical rectangle, scale and primary flag, and its GDI device
/// name (`\\.\DISPLAY1`).
fn describe(handle: HMONITOR) -> Result<(Monitor, String)> {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    // SAFETY: `info` is a MONITORINFOEXW whose cbSize says so.
    let ok =
        unsafe { GetMonitorInfoW(handle, std::ptr::from_mut(&mut info).cast::<MONITORINFO>()) };
    if !ok.as_bool() {
        return Err(Error::Platform("GetMonitorInfoW failed".into()));
    }
    let (mut dpi_x, mut dpi_y) = (0, 0);
    // SAFETY: both out-pointers are valid.
    unsafe { GetDpiForMonitor(handle, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) }
        .map_err(|e| platform_error("GetDpiForMonitor", &e))?;
    let scale = scale_for_dpi(dpi_x)
        .ok_or_else(|| Error::Platform(format!("monitor reports {dpi_x} DPI")))?;
    let rect = info.monitorInfo.rcMonitor;
    Ok((
        Monitor {
            physical: physical_rect(rect),
            scale,
            is_primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
        },
        from_wide(&info.szDevice),
    ))
}

/// A Win32 `RECT` (exclusive right and bottom edges) as a [`PhysicalRect`];
/// inverted rectangles become empty.
pub(super) fn physical_rect(rect: RECT) -> PhysicalRect {
    let extent = |from: i32, to: i32| u32::try_from(i64::from(to) - i64::from(from)).unwrap_or(0);
    PhysicalRect {
        origin: chartreuse_core::geometry::PhysicalPoint::new(rect.left, rect.top),
        size: PhysicalSize::new(extent(rect.left, rect.right), extent(rect.top, rect.bottom)),
    }
}

/// Friendly monitor names by GDI device name, from the active display paths.
/// Empty if the display configuration cannot be read; callers fall back to a
/// numbered name.
fn friendly_names() -> HashMap<String, String> {
    let mut names = HashMap::new();
    let (mut path_count, mut mode_count) = (0, 0);
    // SAFETY: both out-pointers are valid.
    if unsafe {
        GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
    } != ERROR_SUCCESS
    {
        return names;
    }
    let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
    let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
    // SAFETY: the arrays hold as many elements as the counts say.
    let status = unsafe {
        QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            None,
        )
    };
    if status != ERROR_SUCCESS {
        return names;
    }
    paths.truncate(path_count as usize);

    for path in &paths {
        let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                size: size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                adapterId: path.sourceInfo.adapterId,
                id: path.sourceInfo.id,
            },
            ..Default::default()
        };
        let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME {
            header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                size: size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32,
                adapterId: path.targetInfo.adapterId,
                id: path.targetInfo.id,
            },
            ..Default::default()
        };
        // SAFETY: each packet starts with a header whose type and size match it.
        let found = unsafe {
            DisplayConfigGetDeviceInfo(&mut source.header) == 0
                && DisplayConfigGetDeviceInfo(&mut target.header) == 0
        };
        if !found {
            continue;
        }
        let built_in = [
            DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL,
            DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED,
            DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED,
            DISPLAYCONFIG_OUTPUT_TECHNOLOGY_LVDS,
        ]
        .contains(&target.outputTechnology);
        let name = from_wide(&target.monitorFriendlyDeviceName);
        let name = if built_in {
            "Built-in Display".to_owned()
        } else if name.is_empty() {
            continue;
        } else {
            name
        };
        // With several targets per source (mirroring), the first name wins.
        names
            .entry(from_wide(&source.viewGdiDeviceName))
            .or_insert(name);
    }
    names
}
