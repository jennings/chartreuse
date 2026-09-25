//! macOS: the status item, an `NSStatusItem` in the menu bar.
//!
//! [`MacosStatusItem::install`] adds an item with a template icon and the shared
//! [`MENU`]. Every menu item targets one [`MenuTarget`], an Objective-C object
//! whose action method looks the chosen [`MenuAction`] up by the item's tag (its
//! position in [`MENU`]) and sends it through the [`EventSender`]. The returned
//! [`Registration`] owns the status item and the target (menu items do not retain
//! their target); dropping it removes the item from the menu bar.

use chartreuse_core::{flavor, Error, Result};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSImage, NSMenu, NSMenuItem, NSStatusBar,
    NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{ns_string, NSInteger, NSObject, NSObjectProtocol, NSString};

use crate::event::{self, EventSender, Registration};
use crate::status_item::{MenuAction, MenuEntry, StatusItem, StatusItemHandle, MENU};

/// The macOS [`StatusItem`] backend.
#[derive(Debug, Default)]
pub struct MacosStatusItem;

impl MacosStatusItem {
    pub fn new() -> Self {
        Self
    }
}

impl StatusItem for MacosStatusItem {
    fn install(&self) -> Result<StatusItemHandle> {
        let mtm = MainThreadMarker::new().ok_or_else(|| {
            Error::Platform("the status item must be installed on the main thread".into())
        })?;
        let icon = icon()?;
        let (sender, actions) = event::channel();
        let target = MenuTarget::new(mtm, sender);
        let menu = menu(mtm, &target);

        let installed = Installed {
            item: NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength),
            _target: target,
        };
        // Dropping `installed` on an early return removes the item again.
        let button = installed
            .item
            .button(mtm)
            .ok_or_else(|| Error::Platform("the status item has no button".into()))?;
        button.setImage(Some(&icon));
        button.setToolTip(Some(&NSString::from_str(flavor::DISPLAY_NAME)));
        installed.item.setMenu(Some(&menu));

        use_accessory_policy(mtm);
        tracing::info!("status item installed");
        Ok(StatusItemHandle {
            actions,
            registration: Registration::new(installed),
        })
    }
}

/// An installed status item. Dropping it removes the item from the menu bar.
struct Installed {
    // Declared (and so dropped) before the target, which the menu items point to
    // without retaining it.
    item: Retained<NSStatusItem>,
    _target: Retained<MenuTarget>,
}

impl Drop for Installed {
    fn drop(&mut self) {
        NSStatusBar::systemStatusBar().removeStatusItem(&self.item);
        tracing::info!("status item removed");
    }
}

/// Hides the Dock icon. The bundled app starts as an accessory already
/// (`LSUIElement`); run unbundled (`cargo run`), winit makes it a regular app.
fn use_accessory_policy(mtm: MainThreadMarker) {
    let app = NSApplication::sharedApplication(mtm);
    if app.activationPolicy() != NSApplicationActivationPolicy::Accessory
        && !app.setActivationPolicy(NSApplicationActivationPolicy::Accessory)
    {
        tracing::warn!(
            policy = ?app.activationPolicy(),
            "could not switch to the accessory activation policy"
        );
    }
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and `MenuTarget` does not
    // implement `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = EventSender<MenuAction>]
    /// The target of every status item menu item: forwards the chosen action.
    struct MenuTarget;

    impl MenuTarget {
        /// The action of every menu item built by [`menu`]. AppKit passes the
        /// chosen item as the sender.
        // SAFETY: the signature matches an action method (`- (void)chooseMenuItem:(id)sender`);
        // the sender is taken as an untyped object and checked before use.
        #[unsafe(method(chooseMenuItem:))]
        fn choose_menu_item(&self, sender: Option<&AnyObject>) {
            let Some(item) = sender.and_then(AnyObject::downcast_ref::<NSMenuItem>) else {
                tracing::warn!("ignoring a status item menu choice not sent by a menu item");
                return;
            };
            let Some(action) = action_for_tag(item.tag()) else {
                tracing::warn!("ignoring a status item menu choice with an unknown tag");
                return;
            };
            tracing::debug!(?action, "status item menu choice");
            if !self.ivars().send(action) {
                tracing::debug!(?action, "nothing is listening for status item menu choices");
            }
        }
    }

    unsafe impl NSObjectProtocol for MenuTarget {}
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker, sender: EventSender<MenuAction>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(sender);
        // SAFETY: `init` is NSObject's designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}

/// The placeholder menu bar icon: an SF Symbol, as a template image so the menu
/// bar tints it for light and dark appearances.
fn icon() -> Result<Retained<NSImage>> {
    let icon = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        ns_string!("camera.viewfinder"),
        Some(&NSString::from_str(flavor::DISPLAY_NAME)),
    )
    .ok_or_else(|| Error::Platform("the status item icon symbol is unavailable".into()))?;
    icon.setTemplate(true);
    Ok(icon)
}

/// Builds the [`MENU`], with every action item tagged and aimed at `target`.
fn menu(mtm: MainThreadMarker, target: &MenuTarget) -> Retained<NSMenu> {
    let menu = NSMenu::new(mtm);
    for (tag, entry) in tagged_entries() {
        let item = match entry {
            MenuEntry::Separator => NSMenuItem::separatorItem(mtm),
            MenuEntry::Action(action) => action_item(mtm, action, tag, target),
        };
        menu.addItem(&item);
    }
    menu
}

fn action_item(
    mtm: MainThreadMarker,
    action: MenuAction,
    tag: NSInteger,
    target: &MenuTarget,
) -> Retained<NSMenuItem> {
    // SAFETY: `chooseMenuItem:` is a valid selector, and `target` implements it.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(&action.label()),
            Some(sel!(chooseMenuItem:)),
            ns_string!(""),
        )
    };
    item.setTag(tag);
    // SAFETY: menu items do not retain their target; `Installed` keeps `target`
    // alive until the status item (and with it this item's menu) is gone.
    unsafe { item.setTarget(Some(target.as_ref())) };
    item
}

/// The [`MENU`] entries with their menu item tags: each entry's position.
fn tagged_entries() -> impl Iterator<Item = (NSInteger, MenuEntry)> {
    (0..).zip(MENU.iter().copied())
}

/// The action of the menu item tagged `tag` by [`tagged_entries`], if any.
fn action_for_tag(tag: NSInteger) -> Option<MenuAction> {
    match MENU.get(usize::try_from(tag).ok()?)? {
        MenuEntry::Action(action) => Some(*action),
        MenuEntry::Separator => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_tagged_action_maps_back_to_its_action() {
        let mut tags = HashSet::new();
        for (tag, entry) in tagged_entries() {
            assert!(tags.insert(tag), "tag {tag} is used twice");
            let expected = match entry {
                MenuEntry::Action(action) => Some(action),
                MenuEntry::Separator => None,
            };
            assert_eq!(action_for_tag(tag), expected, "tag {tag}");
        }
    }

    #[test]
    fn tags_outside_the_menu_map_to_no_action() {
        let past_the_end = NSInteger::try_from(MENU.len()).unwrap();
        for tag in [-1, past_the_end, NSInteger::MIN, NSInteger::MAX] {
            assert_eq!(action_for_tag(tag), None, "tag {tag}");
        }
    }
}
