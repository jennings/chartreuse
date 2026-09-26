//! Windows: display and window capture.
//!
//! # Flow
//!
//! On the calling (main) thread the displays are enumerated, or the window is
//! looked up; then one worker thread per request, initialized for WinRT, does the
//! capture and resolves the returned future through a oneshot channel.
//!
//! # Windows.Graphics.Capture
//!
//! The primary path. `IGraphicsCaptureItemInterop` makes a `GraphicsCaptureItem`
//! for a monitor (`CreateForMonitor`) or a window (`CreateForWindow`). A
//! free-threaded `Direct3D11CaptureFramePool` receives the first frame, whose
//! texture is copied to a CPU-readable staging texture and converted from BGRA.
//! Cursor capture and (on Windows 11) the yellow capture border are turned off
//! where the OS allows it.
//!
//! # GDI fallbacks
//!
//! Used when Windows.Graphics.Capture is unavailable (`IsSupported`) or fails, or
//! when no frame arrives within [`FRAME_TIMEOUT`]:
//!
//! - Displays: `BitBlt` from the screen DC over the monitor's rectangle, with
//!   `CAPTUREBLT` so layered windows are included.
//! - Windows: `PrintWindow` with `PW_RENDERFULLCONTENT` (which also renders
//!   DirectComposition content), cropped from the window rectangle to the visible
//!   frame.
//!
//! # What a window capture contains
//!
//! The window's visible frame — DWM's extended frame bounds, as the window list
//! reports — without the drop shadow (DWM draws shadows outside the window, and
//! neither API captures them) and without the invisible resize borders. On
//! Windows 11 the rounded corners come out transparent. Other windows covering it
//! are not captured, except in the `PrintWindow` fallback of windows that do not
//! render themselves off screen.
//!
//! # Own windows
//!
//! Unlike macOS, the capture APIs cannot exclude a process's windows. Overlay
//! windows exclude themselves (`WDA_EXCLUDEFROMCAPTURE`, see
//! `overlay_style.rs`); other Chartreuse windows on screen are captured.
//!
//! No permission is involved, so capture never fails with
//! [`Error::PermissionDenied`].

use std::ffi::c_void;
use std::time::{Duration, Instant};

use ::windows::core::{factory, Interface};
use ::windows::Graphics::Capture::{
    Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use ::windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use ::windows::Graphics::DirectX::DirectXPixelFormat;
use ::windows::Win32::Foundation::{HMODULE, HWND, RECT};
use ::windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use ::windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use ::windows::Win32::Graphics::Dxgi::IDXGIDevice;
use ::windows::Win32::Graphics::Gdi::HMONITOR;
use ::windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS,
    HBITMAP, HDC, HGDIOBJ, SRCCOPY,
};
use ::windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use ::windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use ::windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use ::windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};
use ::windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsWindow, PW_RENDERFULLCONTENT};
use chartreuse_core::display::DisplayInfo;
use chartreuse_core::geometry::{PhysicalRect, PhysicalSize};
use chartreuse_core::image::Image;
use chartreuse_core::window::WindowId;
use chartreuse_core::{Error, Result};
use chartreuse_imaging::region::crop;
use futures::channel::oneshot;
use futures::future::{self, BoxFuture, FutureExt};

use super::displays::{enumerate, physical_rect};
use super::pixels::{frame_within, image_from_bgra, visible_bounds, Alpha};
use super::util::platform_error;
use super::window_list::frame_bounds;
use crate::capture::{Capture, DisplayCapture};

/// How long to wait for Windows.Graphics.Capture's first frame before falling
/// back to GDI.
const FRAME_TIMEOUT: Duration = Duration::from_secs(1);

/// The Windows [`Capture`] backend.
#[derive(Debug, Default)]
pub struct WindowsCapture;

impl WindowsCapture {
    pub fn new() -> Self {
        Self
    }
}

impl Capture for WindowsCapture {
    fn capture_displays(&self) -> BoxFuture<'static, Result<Vec<DisplayCapture>>> {
        let monitors = match enumerate() {
            Ok(monitors) => monitors,
            Err(error) => return future::ready(Err(error)).boxed(),
        };
        let targets: Vec<(Handle, PhysicalRect, DisplayInfo)> = monitors
            .native
            .into_iter()
            .zip(monitors.displays)
            .map(|((monitor, rect), display)| (Handle::of(monitor.0), rect, display))
            .collect();
        on_capture_thread(move || {
            let wgc = Wgc::new();
            targets
                .into_iter()
                .map(|(monitor, rect, display)| {
                    let image = capture_display(wgc.as_ref(), monitor, rect, &display)?;
                    Ok(DisplayCapture { display, image })
                })
                .collect()
        })
    }

    fn capture_window(&self, window: WindowId) -> BoxFuture<'static, Result<Image>> {
        let Ok(raw) = usize::try_from(window.0) else {
            return future::ready(Err(window_gone())).boxed();
        };
        let hwnd = HWND(raw as *mut c_void);
        // SAFETY: IsWindow accepts any value.
        if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            return future::ready(Err(window_gone())).boxed();
        }
        let handle = Handle::of(hwnd.0);
        on_capture_thread(move || capture_window(handle, Wgc::new().as_ref()))
    }
}

fn window_gone() -> Error {
    Error::Platform("the window no longer exists".into())
}

/// An `HMONITOR` or `HWND` as a plain number, so it can move to the capture
/// thread. Both are process-independent identifiers, not pointers into memory.
#[derive(Debug, Clone, Copy)]
struct Handle(usize);

impl Handle {
    fn of(raw: *mut c_void) -> Self {
        Self(raw as usize)
    }

    fn monitor(self) -> HMONITOR {
        HMONITOR(self.0 as *mut c_void)
    }

    fn window(self) -> HWND {
        HWND(self.0 as *mut c_void)
    }
}

/// Runs `capture` on a new thread initialized for WinRT and resolves with its
/// result.
fn on_capture_thread<T: Send + 'static>(
    capture: impl FnOnce() -> Result<T> + Send + 'static,
) -> BoxFuture<'static, Result<T>> {
    let (sender, receiver) = oneshot::channel();
    let spawned = std::thread::Builder::new()
        .name("chartreuse-capture".into())
        .spawn(move || {
            // SAFETY: balanced by RoUninitialize below when it succeeds.
            let initialized = unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok();
            let result = capture();
            if initialized {
                // SAFETY: this thread's successful RoInitialize.
                unsafe { RoUninitialize() };
            }
            // The receiver is gone only if the caller dropped the future.
            let _ = sender.send(result);
        });
    if let Err(error) = spawned {
        return future::ready(Err(Error::io("starting the capture thread", error))).boxed();
    }
    receiver
        .map(|result| {
            result.unwrap_or_else(|_| Err(Error::Platform("the capture thread panicked".into())))
        })
        .boxed()
}

// ---------------------------------------------------------------------------
// Displays and windows
// ---------------------------------------------------------------------------

/// Captures one monitor, through Windows.Graphics.Capture if it produces a frame
/// of the display's size, else through `BitBlt`.
fn capture_display(
    wgc: Option<&Wgc>,
    monitor: Handle,
    rect: PhysicalRect,
    info: &DisplayInfo,
) -> Result<Image> {
    if let Some(wgc) = wgc {
        // SAFETY: the interop call only reads the handle; a stale one fails.
        let item = unsafe {
            wgc.interop
                .CreateForMonitor::<GraphicsCaptureItem>(monitor.monitor())
        };
        match item.and_then(|item| wgc.capture(&item, Alpha::Opaque)) {
            Ok(image) if image.size() == info.pixel_size => return Ok(image),
            Ok(image) => tracing::warn!(
                display = info.name,
                captured = ?image.size(),
                expected = ?info.pixel_size,
                "Windows.Graphics.Capture returned a frame of the wrong size; using BitBlt"
            ),
            Err(error) => tracing::warn!(
                display = info.name,
                %error,
                "Windows.Graphics.Capture failed; using BitBlt"
            ),
        }
    }
    capture_screen_rect(rect)
}

/// Captures one window's visible frame (see the module docs).
fn capture_window(window: Handle, wgc: Option<&Wgc>) -> Result<Image> {
    let hwnd = window.window();
    if let Some(wgc) = wgc {
        // SAFETY: the interop call only reads the handle; a stale one fails.
        let item = unsafe { wgc.interop.CreateForWindow::<GraphicsCaptureItem>(hwnd) };
        match item.and_then(|item| wgc.capture(&item, Alpha::Premultiplied)) {
            // The frame can include transparent margins; trim them.
            Ok(image) => match visible_bounds(&image) {
                Some(bounds) => return crop(&image, bounds),
                None => tracing::warn!(
                    "Windows.Graphics.Capture returned a blank window; using PrintWindow"
                ),
            },
            Err(error) => {
                tracing::warn!(%error, "Windows.Graphics.Capture failed; using PrintWindow");
            }
        }
    }
    print_window(hwnd)
}

// ---------------------------------------------------------------------------
// Windows.Graphics.Capture
// ---------------------------------------------------------------------------

/// A Direct3D device and the interop factory for capture items.
struct Wgc {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    winrt_device: IDirect3DDevice,
    interop: IGraphicsCaptureItemInterop,
}

impl Wgc {
    /// `None` (after logging why) if Windows.Graphics.Capture is unavailable.
    fn new() -> Option<Self> {
        match GraphicsCaptureSession::IsSupported() {
            Ok(true) => {}
            Ok(false) => {
                tracing::info!("Windows.Graphics.Capture is not supported; using GDI");
                return None;
            }
            Err(error) => {
                tracing::warn!(%error, "Windows.Graphics.Capture is unavailable; using GDI");
                return None;
            }
        }
        Self::create()
            .inspect_err(|error| {
                tracing::warn!(%error, "could not set up Windows.Graphics.Capture; using GDI");
            })
            .ok()
    }

    fn create() -> ::windows::core::Result<Self> {
        let mut device = None;
        let mut context = None;
        // SAFETY: the out-pointers are valid; no adapter or software module.
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
        }?;
        let (Some(device), Some(context)) = (device, context) else {
            return Err(::windows::core::Error::empty());
        };
        let dxgi: IDXGIDevice = device.cast()?;
        // SAFETY: `dxgi` is a live DXGI device.
        let winrt_device: IDirect3DDevice =
            unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }?.cast()?;
        let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        Ok(Self {
            device,
            context,
            winrt_device,
            interop,
        })
    }

    /// Captures the first frame of `item`.
    fn capture(&self, item: &GraphicsCaptureItem, alpha: Alpha) -> ::windows::core::Result<Image> {
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &self.winrt_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            1,
            item.Size()?,
        )?;
        let session = pool.CreateCaptureSession(item)?;
        // Both need newer Windows versions; without them the capture still works.
        let _ = session.SetIsCursorCaptureEnabled(false);
        let _ = session.SetIsBorderRequired(false);
        session.StartCapture()?;
        let frame = next_frame(&pool);
        let image = frame.and_then(|frame| {
            let image = self.read_frame(&frame, alpha);
            let _ = frame.Close();
            image
        });
        let _ = session.Close();
        let _ = pool.Close();
        image
    }

    /// Copies `frame` to the CPU and converts it.
    fn read_frame(
        &self,
        frame: &Direct3D11CaptureFrame,
        alpha: Alpha,
    ) -> ::windows::core::Result<Image> {
        let content = frame.ContentSize()?;
        let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
        // SAFETY: the surface is a live Direct3D 11 texture.
        let texture: ID3D11Texture2D = unsafe { access.GetInterface() }?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: `desc` is a writable descriptor.
        unsafe { texture.GetDesc(&mut desc) };
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        desc.MiscFlags = 0;
        let mut staging = None;
        // SAFETY: `desc` describes a valid staging texture; the out-pointer is valid.
        unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut staging)) }?;
        let staging = staging.ok_or_else(::windows::core::Error::empty)?;
        // SAFETY: both textures belong to `self.device` and have the same layout.
        unsafe { self.context.CopyResource(&staging, &texture) };

        let size = PhysicalSize::new(
            u32::try_from(content.Width).unwrap_or(0).min(desc.Width),
            u32::try_from(content.Height).unwrap_or(0).min(desc.Height),
        );
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: the staging texture is CPU-readable; the out-pointer is valid.
        unsafe {
            self.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        }?;
        let row_pitch = mapped.RowPitch as usize;
        // SAFETY: a mapped texture of `desc.Height` rows spans `RowPitch × Height`
        // bytes at `pData` until it is unmapped, below.
        let bytes = unsafe {
            std::slice::from_raw_parts(mapped.pData.cast::<u8>(), row_pitch * desc.Height as usize)
        };
        let image = image_from_bgra(bytes, row_pitch, size, alpha);
        // SAFETY: mapped above.
        unsafe { self.context.Unmap(&staging, 0) };
        image.map_err(|error| {
            ::windows::core::Error::new(::windows::core::HRESULT(-1), error.to_string())
        })
    }
}

/// Waits up to [`FRAME_TIMEOUT`] for the pool's first frame.
fn next_frame(
    pool: &Direct3D11CaptureFramePool,
) -> ::windows::core::Result<Direct3D11CaptureFrame> {
    let deadline = Instant::now() + FRAME_TIMEOUT;
    loop {
        // A free-threaded pool fills itself; TryGetNextFrame fails until it has.
        if let Ok(frame) = pool.TryGetNextFrame() {
            return Ok(frame);
        }
        if Instant::now() >= deadline {
            return Err(::windows::core::Error::new(
                ::windows::core::HRESULT(-1),
                "no frame arrived",
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

// ---------------------------------------------------------------------------
// GDI
// ---------------------------------------------------------------------------

/// Captures `rect` of the screen with `BitBlt`.
fn capture_screen_rect(rect: PhysicalRect) -> Result<Image> {
    let screen = ScreenDc::get()?;
    let bitmap = MemoryBitmap::new(&screen, rect.size)?;
    let (width, height) = bitmap.extent();
    // SAFETY: both DCs are live; the bitmap is selected into the memory DC.
    unsafe {
        BitBlt(
            bitmap.dc,
            0,
            0,
            width,
            height,
            Some(screen.0),
            rect.min_x(),
            rect.min_y(),
            SRCCOPY | CAPTUREBLT,
        )
    }
    .map_err(|e| platform_error("BitBlt", &e))?;
    bitmap.read()
}

/// Renders `hwnd` with `PrintWindow` and crops it to its visible frame.
fn print_window(hwnd: HWND) -> Result<Image> {
    let mut window_rect = RECT::default();
    // SAFETY: `window_rect` is writable.
    unsafe { GetWindowRect(hwnd, &mut window_rect) }.map_err(|_| window_gone())?;
    let window_rect = physical_rect(window_rect);
    let frame = frame_bounds(hwnd).unwrap_or(window_rect);
    let screen = ScreenDc::get()?;
    let bitmap = MemoryBitmap::new(&screen, window_rect.size)?;
    // SAFETY: the memory DC is live and has the bitmap selected.
    let printed = unsafe { PrintWindow(hwnd, bitmap.dc, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)) };
    if !printed.as_bool() {
        return Err(Error::Platform("PrintWindow failed".into()));
    }
    let image = bitmap.read()?;
    let visible = frame_within(window_rect, frame)
        .ok_or_else(|| Error::Platform("the window has no visible frame".into()))?;
    crop(&image, visible)
}

/// The screen DC, released on drop.
struct ScreenDc(HDC);

impl ScreenDc {
    fn get() -> Result<Self> {
        // SAFETY: no preconditions; a null DC is checked below.
        let dc = unsafe { GetDC(None) };
        if dc.is_invalid() {
            return Err(Error::Platform("GetDC failed".into()));
        }
        Ok(Self(dc))
    }
}

impl Drop for ScreenDc {
    fn drop(&mut self) {
        // SAFETY: acquired by GetDC(None).
        unsafe { ReleaseDC(None, self.0) };
    }
}

/// A 32-bit bitmap compatible with the screen, selected into its own memory DC
/// until it is read; both are freed on drop.
struct MemoryBitmap {
    dc: HDC,
    bitmap: HBITMAP,
    /// The DC's original bitmap while `bitmap` is selected in its place.
    previous: Option<HGDIOBJ>,
    size: PhysicalSize,
}

impl MemoryBitmap {
    fn new(screen: &ScreenDc, size: PhysicalSize) -> Result<Self> {
        let too_large = || {
            Error::Platform(format!(
                "cannot capture {}×{} pixels",
                size.width, size.height
            ))
        };
        let width = i32::try_from(size.width).map_err(|_| too_large())?;
        let height = i32::try_from(size.height).map_err(|_| too_large())?;
        if size.is_empty() {
            return Err(too_large());
        }
        // SAFETY: `screen` is a live DC; failures return null handles, checked.
        unsafe {
            let dc = CreateCompatibleDC(Some(screen.0));
            if dc.is_invalid() {
                return Err(Error::Platform("CreateCompatibleDC failed".into()));
            }
            let bitmap = CreateCompatibleBitmap(screen.0, width, height);
            if bitmap.is_invalid() {
                let _ = DeleteDC(dc);
                return Err(Error::Platform("CreateCompatibleBitmap failed".into()));
            }
            let previous = SelectObject(dc, bitmap.into());
            Ok(Self {
                dc,
                bitmap,
                previous: Some(previous),
                size,
            })
        }
    }

    fn extent(&self) -> (i32, i32) {
        // Checked to fit in `new`.
        (self.size.width as i32, self.size.height as i32)
    }

    /// Selects the DC's original bitmap back in, if `bitmap` is still selected.
    fn deselect(&mut self) {
        if let Some(previous) = self.previous.take() {
            // SAFETY: `previous` is what `new` swapped out of this live DC.
            unsafe { SelectObject(self.dc, previous) };
        }
    }

    /// The bitmap's pixels, as an opaque image.
    fn read(mut self) -> Result<Image> {
        // GetDIBits requires the bitmap not to be selected into any DC.
        self.deselect();
        let (width, height) = self.extent();
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                // Negative: top-down rows.
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let row_pitch = self.size.width as usize * 4;
        let mut bgra = vec![0u8; row_pitch * self.size.height as usize];
        // SAFETY: `bgra` holds `height` rows of 32-bit pixels, as `info` describes;
        // the bitmap was deselected above.
        let rows = unsafe {
            GetDIBits(
                self.dc,
                self.bitmap,
                0,
                self.size.height,
                Some(bgra.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            )
        };
        if rows != height {
            return Err(Error::Platform("GetDIBits failed".into()));
        }
        image_from_bgra(&bgra, row_pitch, self.size, Alpha::Opaque)
    }
}

impl Drop for MemoryBitmap {
    fn drop(&mut self) {
        self.deselect();
        // SAFETY: created in `new` and not freed elsewhere; `deselect` (here or
        // in `read`) has put the DC's original bitmap back, so `bitmap` is not
        // selected when it is deleted.
        unsafe {
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}
