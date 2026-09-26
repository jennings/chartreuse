//! Wayland: display enumeration from the `wl_output` globals and their
//! `xdg-output` logical geometry (see [`crate::linux::logic::wl_output`] for
//! how fractional scales follow from them).
//!
//! Each call opens a short-lived connection of its own to the compositor,
//! binds every output, and reads their descriptions in two round trips; the
//! connection closes when the call returns.

use chartreuse_core::display::DisplayInfo;
use chartreuse_core::geometry::PhysicalSize;
use chartreuse_core::{Error, Result};
use wayland_client::globals::GlobalListContents;
use wayland_client::protocol::wl_output::{self, Transform, WlOutput};
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{delegate_noop, Connection, Dispatch, Proxy, QueueHandle, WEnum};
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_manager_v1::ZxdgOutputManagerV1;
use wayland_protocols::xdg::xdg_output::zv1::client::zxdg_output_v1::{self, ZxdgOutputV1};

use crate::displays::Displays;
use crate::linux::logic::wl_output::{self as outputs, Output};

/// The Wayland [`Displays`] backend.
#[derive(Debug, Default)]
pub struct WaylandDisplays;

impl WaylandDisplays {
    pub fn new() -> Self {
        Self
    }
}

impl Displays for WaylandDisplays {
    fn displays(&self) -> Result<Vec<DisplayInfo>> {
        query()
    }
}

/// Every output the compositor announces, as displays.
pub fn query() -> Result<Vec<DisplayInfo>> {
    let failed = |error: &dyn std::fmt::Display| {
        Error::Platform(format!("Wayland: listing the outputs failed: {error}"))
    };
    let conn = Connection::connect_to_env().map_err(|e| failed(&e))?;
    let (globals, mut queue) =
        wayland_client::globals::registry_queue_init::<State>(&conn).map_err(|e| failed(&e))?;
    let qh = queue.handle();
    let mut state = State::default();
    for global in globals.contents().clone_list() {
        if global.interface == WlOutput::interface().name {
            let version = global.version.min(4);
            let output: WlOutput = globals
                .registry()
                .bind(global.name, version, &qh, global.name);
            state.outputs.push(Tracked {
                output,
                xdg: None,
                info: Output {
                    id: global.name,
                    integer_scale: 1,
                    ..Output::default()
                },
            });
        }
    }
    let manager: Option<ZxdgOutputManagerV1> = globals.bind(&qh, 1..=3, ()).ok();
    if let Some(manager) = &manager {
        for tracked in &mut state.outputs {
            let id = tracked.info.id;
            tracked.xdg = Some(manager.get_xdg_output(&tracked.output, &qh, id));
        }
    }
    // The first round trip delivers the outputs' and xdg-outputs' initial
    // events.
    queue.roundtrip(&mut state).map_err(|e| failed(&e))?;
    let outputs: Vec<Output> = state
        .outputs
        .iter()
        .map(|tracked| tracked.info.clone())
        .collect();
    for tracked in state.outputs {
        if let Some(xdg) = tracked.xdg {
            xdg.destroy();
        }
        if tracked.output.version() >= 3 {
            tracked.output.release();
        }
    }
    if let Some(manager) = manager {
        manager.destroy();
    }
    let _ = conn.flush();
    let displays = outputs::displays(&outputs);
    if displays.is_empty() {
        return Err(Error::Platform(
            "Wayland: the compositor announces no outputs".into(),
        ));
    }
    Ok(displays)
}

#[derive(Debug, Default)]
struct State {
    outputs: Vec<Tracked>,
}

#[derive(Debug)]
struct Tracked {
    output: WlOutput,
    xdg: Option<ZxdgOutputV1>,
    info: Output,
}

impl State {
    fn output(&mut self, id: u32) -> Option<&mut Output> {
        self.outputs
            .iter_mut()
            .find(|tracked| tracked.info.id == id)
            .map(|tracked| &mut tracked.info)
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Outputs plugged in while listing are picked up by the next call.
    }
}

impl Dispatch<WlOutput, u32> for State {
    fn event(
        state: &mut Self,
        _: &WlOutput,
        event: wl_output::Event,
        id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(output) = state.output(*id) else {
            return;
        };
        match event {
            wl_output::Event::Geometry {
                x, y, transform, ..
            } => {
                output.position = (x, y);
                output.rotated = matches!(
                    transform,
                    WEnum::Value(
                        Transform::_90
                            | Transform::_270
                            | Transform::Flipped90
                            | Transform::Flipped270
                    )
                );
            }
            wl_output::Event::Mode {
                flags,
                width,
                height,
                ..
            } => {
                let current = matches!(flags, WEnum::Value(flags) if flags.contains(wl_output::Mode::Current));
                if let (true, Ok(width), Ok(height)) =
                    (current, u32::try_from(width), u32::try_from(height))
                {
                    output.mode = Some(PhysicalSize::new(width, height));
                }
            }
            wl_output::Event::Scale { factor } => output.integer_scale = factor,
            wl_output::Event::Name { name } => output.name = Some(name),
            wl_output::Event::Description { description } => {
                output.description = Some(description);
            }
            _ => {}
        }
    }
}

impl Dispatch<ZxdgOutputV1, u32> for State {
    fn event(
        state: &mut Self,
        _: &ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        id: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(output) = state.output(*id) else {
            return;
        };
        match event {
            zxdg_output_v1::Event::LogicalPosition { x, y } => {
                output.logical.get_or_insert_default().0 = (x, y);
            }
            zxdg_output_v1::Event::LogicalSize { width, height } => {
                output.logical.get_or_insert_default().1 = (width, height);
            }
            zxdg_output_v1::Event::Name { name } => {
                output.name.get_or_insert(name);
            }
            zxdg_output_v1::Event::Description { description } => {
                output.description.get_or_insert(description);
            }
            _ => {}
        }
    }
}

delegate_noop!(State: ignore ZxdgOutputManagerV1);
