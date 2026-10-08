use super::*;

fn key_event(chars: &str, unmod: &str, flags: NSEventModifierFlags, code: u16) -> id {
    let chars = nsstring(chars);
    let unmod = nsstring(unmod);
    unsafe {
        NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode_(
            nil,
            appkit::NSEventType::NSKeyDown,
            NSPoint::new(0., 0.),
            flags,
            0.,
            0,
            nil,
            *chars,
            *unmod,
            NO,
            code,
        )
    }
}

#[test]
fn vtabs_menu_shortcuts_offer_the_actual_chord_before_the_assignment() {
    let _pool = unsafe { StrongPtr::new(NSAutoreleasePool::new(nil)) };
    for (chars, unmod, flags, code, key, mods) in [
        (
            "1",
            "1",
            NSEventModifierFlags::NSCommandKeyMask,
            kVK_ANSI_1,
            KeyCode::Char('1'),
            Modifiers::SUPER,
        ),
        (
            "!",
            "!",
            NSEventModifierFlags::NSCommandKeyMask | NSEventModifierFlags::NSShiftKeyMask,
            kVK_ANSI_1,
            KeyCode::Char('!'),
            Modifiers::SUPER | Modifiers::SHIFT,
        ),
        (
            "\u{3}",
            "c",
            NSEventModifierFlags::NSControlKeyMask,
            kVK_ANSI_C,
            KeyCode::Char('c'),
            Modifiers::CTRL,
        ),
        (
            "\u{f702}",
            "\u{f702}",
            NSEventModifierFlags::NSCommandKeyMask,
            kVK_LeftArrow,
            KeyCode::LeftArrow,
            Modifiers::SUPER,
        ),
    ] {
        let event = key_event(chars, unmod, flags, code);
        assert!(!event.is_null());
        let mut calls = 0;
        assert!(dispatch_menu_key_equivalent(event, |event| {
            let WindowEvent::RawKeyEvent(event) = event else {
                panic!("expected raw input before assignment")
            };
            calls += 1;
            assert_eq!(event.key, key);
            assert_eq!(event.modifiers, mods);
            assert_eq!(event.phys_code, vkey_to_phys(code));
            assert!(event.key_is_down);
            event.set_handled();
        }));
        assert_eq!(calls, 1);
    }
}

#[test]
fn vtabs_menu_shortcuts_allow_unclaimed_bindings_to_execute() {
    let _pool = unsafe { StrongPtr::new(NSAutoreleasePool::new(nil)) };
    let event = key_event("1", "1", NSEventModifierFlags::NSCommandKeyMask, kVK_ANSI_1);
    let mut calls = 0;
    assert!(!dispatch_menu_key_equivalent(event, |_| calls += 1));
    assert_eq!(calls, 1);
}

#[test]
fn vtabs_menu_mouse_clicks_and_programmatic_actions_do_not_synthesize_keys() {
    let _pool = unsafe { StrongPtr::new(NSAutoreleasePool::new(nil)) };
    let mouse = unsafe {
        NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure_(
            nil, appkit::NSEventType::NSLeftMouseUp, NSPoint::new(0., 0.),
            NSEventModifierFlags::empty(), 0., 0, nil, 0, 1, 0.,
        )
    };
    assert!(!mouse.is_null());
    for event in [nil, mouse] {
        assert!(!dispatch_menu_key_equivalent(event, |_| panic!(
            "not a keyboard shortcut"
        )));
    }
}
