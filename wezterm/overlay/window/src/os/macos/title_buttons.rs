//! Native title buttons, titlebar drags and menu key equivalents for the sidebar.
use super::*;
use cocoa::appkit::{NSView, NSWindow, NSWindowButton};
use cocoa::base::{id, nil, BOOL, YES};
use cocoa::foundation::{NSArray, NSPoint, NSRect};
use objc::{class, msg_send, sel, sel_impl};

use super::super::nsstring;

#[link(name = "CoreImage", kind = "framework")]
extern "C" {}

/// Development builds reopen where the last one closed.
pub(super) fn restore_dev_frame(window: id) {
    if std::env::var_os("WEZ_VTABS_DEV").is_none() {
        return;
    }
    unsafe {
        let autosave_name = *nsstring("vtabs-dev");
        let _: bool = msg_send![window, setFrameUsingName: autosave_name];
        let _: bool = msg_send![window, setFrameAutosaveName: autosave_name];
    }
}

pub(super) fn hide(window: &StrongPtr, hidden: bool) {
    unsafe {
        for titlebar_button in &[
            appkit::NSWindowButton::NSWindowMiniaturizeButton,
            appkit::NSWindowButton::NSWindowCloseButton,
            appkit::NSWindowButton::NSWindowZoomButton,
        ] {
            let button = window.standardWindowButton_(*titlebar_button);
            let _: () = msg_send![button, setHidden: to_yes_no(hidden)];
        }
        update(**window, None);
    }
}

/// Window-level events also cover the native titlebar and the gaps between its buttons.
pub(super) extern "C" fn send_event(this: &mut Object, _sel: Sel, event: id) {
    unsafe {
        use appkit::NSEventType::*;
        if matches!(
            event.eventType(),
            NSMouseMoved
                | NSMouseEntered
                | NSMouseExited
                | NSLeftMouseDown
                | NSLeftMouseUp
                | NSLeftMouseDragged
                | NSRightMouseDown
                | NSRightMouseUp
                | NSRightMouseDragged
                | NSOtherMouseDown
                | NSOtherMouseUp
                | NSOtherMouseDragged
        ) {
            update(this as *mut Object, Some(event.locationInWindow()));
        }
        let _: () = msg_send![super(this, class!(NSWindow)), sendEvent: event];
    }
}

/// Offers the actual chord to input consumers before its menu's key assignment.
pub(super) fn dispatch_menu_key_equivalent(
    event: id,
    mut dispatch: impl FnMut(WindowEvent),
) -> bool {
    if event.is_null() || unsafe { event.eventType() } != appkit::NSEventType::NSKeyDown {
        return false;
    }
    let flags = unsafe { event.modifierFlags() };
    let virtual_key = unsafe { event.keyCode() };
    let unmod = unsafe { nsstring_to_str(event.charactersIgnoringModifiers()) };
    let phys_code = vkey_to_phys(virtual_key);
    let key = match KeyCode::composed(unmod) {
        KeyCode::Char(c) => function_key_to_keycode(c),
        _ if unmod.is_empty() => phys_code
            .map(KeyCode::Physical)
            .unwrap_or(KeyCode::RawCode(virtual_key as _)),
        key => key,
    };
    let handled = Handled::new();
    dispatch(WindowEvent::RawKeyEvent(RawKeyEvent {
        key,
        phys_code,
        raw_code: virtual_key as _,
        leds: if flags.contains(NSEventModifierFlags::NSAlphaShiftKeyMask) {
            KeyboardLedStatus::CAPS_LOCK
        } else {
            KeyboardLedStatus::empty()
        },
        modifiers: key_modifiers(flags),
        repeat_count: 1,
        key_is_down: true,
        handled: handled.clone(),
    }));
    handled.is_handled()
}

impl WindowInner {
    pub(super) fn set_window_drag_exclusion(&mut self, content: Rect) {
        if let Some(window_view) = WindowView::get_this(unsafe { &**self.view }) {
            window_view.inner.borrow_mut().drag_exclusion = Some(content);
        }
    }

    pub(super) fn set_title_buttons_hidden(&mut self, hidden: bool) {
        if self.title_buttons_hidden != hidden {
            self.title_buttons_hidden = hidden;
            let decorations = effective_decorations(
                self.config.window_decorations,
                self.config.integrated_title_button_style,
            );
            let decorated = decorations
                .intersects(WindowDecorations::TITLE | WindowDecorations::INTEGRATED_BUTTONS);
            hide(&self.window, hidden || !decorated);
        }
    }
}

impl WindowView {
    /// AppKit menu shortcuts bypass keyDown.
    pub(super) fn dispatch_menu_key_equivalent(&self) -> bool {
        let event: id = unsafe { msg_send![appkit::NSApp(), currentEvent] };
        let mut inner = self.inner.borrow_mut();
        dispatch_menu_key_equivalent(event, |event| inner.events.dispatch(event))
    }

    /// Keeps a transparent titlebar from dragging or zooming the window over excluded content.
    pub(super) extern "C" fn mouse_down_can_move_window(this: &Object, _sel: Sel) -> BOOL {
        let content =
            Self::get_this(this).and_then(|myself| myself.inner.try_borrow().ok()?.drag_exclusion);
        let event: id = unsafe { msg_send![appkit::NSApp(), currentEvent] };
        match content {
            Some(content) if !event.is_null() => {
                let point = backing_point(this as *const Object as id, event);
                to_yes_no(!content.contains(Point::new(point.x as isize, point.y as isize)))
            }
            _ => YES,
        }
    }
}

/// As `WindowView::mouse_common` maps events onto the view's backing pixels.
fn backing_point(view: id, nsevent: id) -> NSPoint {
    unsafe {
        let point = NSView::convertPoint_fromView_(view, nsevent.locationInWindow(), nil);
        let rect = NSRect::new(NSPoint::new(0., 0.), NSSize::new(point.x, point.y));
        let backing_rect = NSView::convertRectToBacking(view, rect);
        // The backing rect is unsigned; the point's signs carry over.
        NSPoint::new(
            f64::copysign(backing_rect.size.width, point.x),
            f64::copysign(backing_rect.size.height, point.y),
        )
    }
}

#[cfg(test)]
#[path = "../../../tests/macos/menu.rs"]
mod menu_tests;

/// Native buttons keep their hit targets and menu, muted monochrome at rest and native on hover.
pub(super) fn update(window: id, position: Option<NSPoint>) {
    unsafe {
        let buttons = [
            window.standardWindowButton_(NSWindowButton::NSWindowCloseButton),
            window.standardWindowButton_(NSWindowButton::NSWindowMiniaturizeButton),
            window.standardWindowButton_(NSWindowButton::NSWindowZoomButton),
        ];
        let mut group: Option<NSRect> = None;
        for button in buttons {
            if button.is_null() {
                continue;
            }
            let hidden: BOOL = msg_send![button, isHiddenOrHasHiddenAncestor];
            if hidden == YES {
                continue;
            }
            let rect: NSRect = msg_send![button, convertRect: button.bounds() toView: nil];
            group = Some(match group {
                None => rect,
                Some(group) => {
                    let left = group.origin.x.min(rect.origin.x);
                    let bottom = group.origin.y.min(rect.origin.y);
                    let right =
                        (group.origin.x + group.size.width).max(rect.origin.x + rect.size.width);
                    let top =
                        (group.origin.y + group.size.height).max(rect.origin.y + rect.size.height);
                    NSRect::new(
                        NSPoint::new(left, bottom),
                        cocoa::foundation::NSSize::new(right - left, top - bottom),
                    )
                }
            });
        }
        let point =
            position.unwrap_or_else(|| msg_send![window, mouseLocationOutsideOfEventStream]);
        let hovered = group.is_some_and(|rect| {
            point.x >= rect.origin.x
                && point.x < rect.origin.x + rect.size.width
                && point.y >= rect.origin.y
                && point.y < rect.origin.y + rect.size.height
        });
        let alpha: f64 = if hovered { 1.0 } else { 0.5 };
        for button in buttons {
            if button.is_null() {
                continue;
            }
            let current: f64 = msg_send![button, alphaValue];
            if current == alpha {
                continue;
            }
            let filters = if hovered {
                nil
            } else {
                let filter: id = msg_send![class!(CIFilter),
                    filterWithName: *nsstring("CIColorControls")];
                let _: () = msg_send![filter, setDefaults];
                let zero: id = msg_send![class!(NSNumber), numberWithDouble: 0.0f64];
                let _: () = msg_send![filter, setValue: zero
                    forKey: *nsstring("inputSaturation")];
                NSArray::arrayWithObject(nil, filter)
            };
            let _: () = msg_send![button, setWantsLayer: YES];
            let _: () = msg_send![button, setContentFilters: filters];
            let _: () = msg_send![button, setAlphaValue: alpha];
        }
    }
}
