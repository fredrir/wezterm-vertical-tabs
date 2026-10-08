use cocoa::appkit::{NSView, NSWindow, NSWindowButton};
use cocoa::base::{id, nil, BOOL, YES};
use cocoa::foundation::{NSArray, NSPoint, NSRect};
use objc::{class, msg_send, sel, sel_impl};

use super::super::nsstring;

#[link(name = "CoreImage", kind = "framework")]
extern "C" {}

// Keep the native buttons, including their hit targets and fullscreen menu.
// Only their rendering changes: muted monochrome at rest, native on hover.
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
