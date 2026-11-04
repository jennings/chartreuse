//! Refusing to quit while the status item is installed.
//!
//! A menu bar app keeps running with no windows open, so quitting it the way
//! regular apps quit should not end it: Cmd+Q and the app menu's Quit (which
//! send `terminate:`) and the Dock menu's Quit (a quit Apple event). AppKit
//! asks the app delegate's `applicationShouldTerminate:` before quitting, but
//! winit's delegate does not answer it, so [`QuitRequests::route`] adds the
//! method to the delegate's class. While a [`QuitRequests`] is alive, the
//! method cancels the quit and sends a request through its sender instead.
//!
//! Still quitting: logging out, restarting, and shutting down, whose quit Apple
//! event carries a reason ([`system_is_quitting`]); and `iced::exit` (the status
//! item's Quit), which stops the run loop without `terminate:`.

use std::cell::{Cell, RefCell};
use std::ffi::CString;
use std::sync::LazyLock;

use objc2::ffi::class_addMethod;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Imp, Sel};
use objc2::{msg_send, sel, Encode, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSApplicationTerminateReply};
use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager};

use crate::event::EventSender;

thread_local! {
    /// Where refused quits go: the sender of the newest live [`QuitRequests`],
    /// keyed by its generation.
    static DESTINATION: RefCell<Option<(u64, EventSender<()>)>> = const { RefCell::new(None) };
    /// The generation of the newest [`QuitRequests`].
    static GENERATION: Cell<u64> = const { Cell::new(0) };
    /// Whether [`add_should_terminate`] has run.
    static ADDED: Cell<bool> = const { Cell::new(false) };
}

/// Refuses quits and sends them through a sender, until dropped (on the main
/// thread). A newer one takes over; dropping a replaced one changes nothing.
#[derive(Debug)]
pub(super) struct QuitRequests {
    generation: u64,
}

impl QuitRequests {
    /// Refuses quits from now on, sending each through `sender`.
    pub(super) fn route(mtm: MainThreadMarker, sender: EventSender<()>) -> Self {
        add_should_terminate(mtm);
        let generation = GENERATION.get() + 1;
        GENERATION.set(generation);
        DESTINATION.set(Some((generation, sender)));
        Self { generation }
    }
}

impl Drop for QuitRequests {
    fn drop(&mut self) {
        DESTINATION.with_borrow_mut(|destination| {
            if destination
                .as_ref()
                .is_some_and(|(generation, _)| *generation == self.generation)
            {
                *destination = None;
            }
        });
    }
}

/// `-(NSApplicationTerminateReply)applicationShouldTerminate:(NSApplication *)sender`.
type ShouldTerminate =
    extern "C-unwind" fn(&AnyObject, Sel, &NSApplication) -> NSApplicationTerminateReply;

/// The Objective-C type encoding of [`ShouldTerminate`]. A static because the
/// runtime may keep the pointer for as long as the method exists.
static SHOULD_TERMINATE_TYPES: LazyLock<CString> = LazyLock::new(|| {
    CString::new(format!(
        "{}{}{}{}",
        NSApplicationTerminateReply::ENCODING,
        <&AnyObject>::ENCODING,
        Sel::ENCODING,
        <&NSApplication>::ENCODING,
    ))
    .expect("type encodings have no NUL")
});

/// Adds [`should_terminate`] to the app delegate's class as
/// `applicationShouldTerminate:`, once.
fn add_should_terminate(mtm: MainThreadMarker) {
    if ADDED.replace(true) {
        return;
    }
    let Some(delegate) = NSApplication::sharedApplication(mtm).delegate() else {
        tracing::warn!("the app has no delegate, so Cmd+Q quits the app");
        return;
    };
    let class = AsRef::<AnyObject>::as_ref(&*delegate).class();
    let imp: ShouldTerminate = should_terminate;
    // SAFETY: `class` is the class of a live object. `imp` has the signature
    // of `applicationShouldTerminate:`, which the static
    // `SHOULD_TERMINATE_TYPES` encodes; function pointers of every signature
    // share one representation, and the runtime calls `imp` with that
    // signature.
    let added = unsafe {
        class_addMethod(
            std::ptr::from_ref(class).cast_mut(),
            sel!(applicationShouldTerminate:),
            std::mem::transmute::<ShouldTerminate, Imp>(imp),
            SHOULD_TERMINATE_TYPES.as_ptr(),
        )
    };
    if added.as_bool() {
        tracing::debug!(?class, "refusing quits through the app delegate");
    } else {
        tracing::warn!(
            ?class,
            "the app delegate already answers applicationShouldTerminate:, so Cmd+Q quits the app"
        );
    }
}

/// `applicationShouldTerminate:`: cancels the quit and sends a request while
/// a [`QuitRequests`] is alive, unless the system is quitting the app.
extern "C-unwind" fn should_terminate(
    _delegate: &AnyObject,
    _cmd: Sel,
    _sender: &NSApplication,
) -> NSApplicationTerminateReply {
    if system_is_quitting() {
        tracing::info!("quitting: the user is logging out, restarting, or shutting down");
        return NSApplicationTerminateReply::TerminateNow;
    }
    let requested = DESTINATION.with_borrow(|destination| {
        destination
            .as_ref()
            .is_some_and(|(_, sender)| sender.send(()))
    });
    if requested {
        tracing::info!("refused to quit; the status item's Quit quits");
        NSApplicationTerminateReply::TerminateCancel
    } else {
        NSApplicationTerminateReply::TerminateNow
    }
}

/// `kAEQuitReason` (`'why?'`): the attribute of a quit Apple event saying why
/// the system quits the app (logging out, restarting, shutting down).
const QUIT_REASON: u32 = u32::from_be_bytes(*b"why?");

/// Whether the app is quitting because the user is logging out, restarting, or
/// shutting down: the Apple event being handled, if any, gives a quit reason.
/// Cmd+Q comes with no Apple event.
fn system_is_quitting() -> bool {
    let Some(event) = NSAppleEventManager::sharedAppleEventManager().currentAppleEvent() else {
        return false;
    };
    // SAFETY: `attributeDescriptorForKeyword:` takes an `AEKeyword` (a
    // `FourCharCode`, `u32`) and returns a nullable descriptor.
    let reason: Option<Retained<NSAppleEventDescriptor>> =
        unsafe { msg_send![&*event, attributeDescriptorForKeyword: QUIT_REASON] };
    reason.is_some()
}
