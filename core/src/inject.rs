//! Text injection into the focused field of the frontmost app.
//!
//! macOS   — primary: Accessibility API (set AXSelectedText on the focused
//!           element = insert at cursor); fallback: clipboard + synthetic ⌘V.
//!           Refuses secure text fields (AXSecureTextField).
//! Windows — primary (Paste mode): clipboard + synthetic Ctrl+V; Type mode:
//!           SendInput KEYEVENTF_UNICODE per char. Best-effort password-field
//!           refusal via UI Automation IsPassword.
//! Linux   — dev fallback: sets the clipboard and reports that; used for
//!           development/testing only (targets are Windows 11 + macOS).
//!
//! Paste mode saves and restores the previous clipboard contents.

use crate::settings::OutputMode;
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InjectOutcome {
    Injected,
    /// Text was left on the clipboard for manual paste (Linux dev fallback
    /// or an app that blocks synthetic input).
    ClipboardOnly,
    /// Focused control is a password field; nothing was written anywhere.
    RefusedSecureField,
}

pub fn inject_text(text: &str, mode: OutputMode) -> Result<InjectOutcome> {
    if text.is_empty() {
        return Ok(InjectOutcome::Injected);
    }
    if is_secure_field_focused() {
        return Ok(InjectOutcome::RefusedSecureField);
    }
    platform::inject(text, mode)
}

/// Name of the frontmost application (for history/app-context), best-effort.
pub fn frontmost_app_name() -> String {
    platform::frontmost_app_name()
}

fn is_secure_field_focused() -> bool {
    platform::is_secure_field_focused()
}

/// Sets clipboard, invokes `paste_keystroke`, restores clipboard after a
/// short delay so the paste happens against our text.
fn paste_via_clipboard(text: &str, paste_keystroke: impl Fn() -> Result<()>) -> Result<()> {
    let mut cb = arboard::Clipboard::new()?;
    let saved = cb.get_text().ok();
    cb.set_text(text.to_string())?;
    // Give the clipboard owner change time to propagate before the keystroke.
    std::thread::sleep(std::time::Duration::from_millis(30));
    paste_keystroke()?;
    // Let the target app read the clipboard before restoring.
    std::thread::sleep(std::time::Duration::from_millis(150));
    if let Some(prev) = saved {
        cb.set_text(prev).ok();
    }
    Ok(())
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    const KEY_V: u16 = 9; // kVK_ANSI_V

    pub fn inject(text: &str, _mode: OutputMode) -> Result<InjectOutcome> {
        // AX insert first: no clipboard disturbance, works in most Cocoa apps.
        if ax::insert_via_ax(text) {
            return Ok(InjectOutcome::Injected);
        }
        paste_via_clipboard(text, || {
            let src = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
                .map_err(|_| anyhow::anyhow!("CGEventSource"))?;
            let down = CGEvent::new_keyboard_event(src.clone(), KEY_V, true)
                .map_err(|_| anyhow::anyhow!("CGEvent down"))?;
            down.set_flags(CGEventFlags::CGEventFlagCommand);
            down.post(CGEventTapLocation::HID);
            let up = CGEvent::new_keyboard_event(src, KEY_V, false)
                .map_err(|_| anyhow::anyhow!("CGEvent up"))?;
            up.set_flags(CGEventFlags::CGEventFlagCommand);
            up.post(CGEventTapLocation::HID);
            Ok(())
        })?;
        Ok(InjectOutcome::Injected)
    }

    pub fn is_secure_field_focused() -> bool {
        ax::focused_role()
            .map(|r| r == "AXSecureTextField")
            .unwrap_or(false)
    }

    pub fn frontmost_app_name() -> String {
        ax::frontmost_app_name().unwrap_or_default()
    }

    mod ax {
        //! Minimal AXUIElement calls via accessibility-sys.
        use accessibility_sys::{
            kAXErrorSuccess, kAXFocusedUIElementAttribute, kAXRoleAttribute,
            kAXSelectedTextAttribute, kAXTitleAttribute, AXUIElementCopyAttributeValue,
            AXUIElementCreateSystemWide, AXUIElementRef, AXUIElementSetAttributeValue,
        };
        use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
        use core_foundation::string::{CFString, CFStringRef};

        fn copy_attr(elem: AXUIElementRef, attr: &str) -> Option<CFTypeRef> {
            let name = CFString::new(attr);
            let mut value: CFTypeRef = std::ptr::null();
            let err = unsafe {
                AXUIElementCopyAttributeValue(elem, name.as_concrete_TypeRef(), &mut value)
            };
            (err == kAXErrorSuccess && !value.is_null()).then_some(value)
        }

        fn focused_element() -> Option<AXUIElementRef> {
            unsafe {
                let system = AXUIElementCreateSystemWide();
                let v = copy_attr(system, kAXFocusedUIElementAttribute)?;
                CFRelease(system as CFTypeRef);
                Some(v as AXUIElementRef)
            }
        }

        pub fn focused_role() -> Option<String> {
            unsafe {
                let elem = focused_element()?;
                let role = copy_attr(elem, kAXRoleAttribute);
                CFRelease(elem as CFTypeRef);
                let role = role?;
                let s = CFString::wrap_under_create_rule(role as CFStringRef).to_string();
                Some(s)
            }
        }

        /// Replaces the current selection (empty selection = insert at caret).
        pub fn insert_via_ax(text: &str) -> bool {
            unsafe {
                let Some(elem) = focused_element() else {
                    return false;
                };
                let attr = CFString::new(kAXSelectedTextAttribute);
                let value = CFString::new(text);
                let err = AXUIElementSetAttributeValue(
                    elem,
                    attr.as_concrete_TypeRef(),
                    value.as_CFTypeRef(),
                );
                CFRelease(elem as CFTypeRef);
                err == kAXErrorSuccess
            }
        }

        pub fn frontmost_app_name() -> Option<String> {
            // Title of the focused element's app is unreliable; use the
            // focused element's top-level title as best-effort.
            unsafe {
                let elem = focused_element()?;
                let title = copy_attr(elem, kAXTitleAttribute);
                CFRelease(elem as CFTypeRef);
                let title = title?;
                Some(CFString::wrap_under_create_rule(title as CFStringRef).to_string())
            }
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_CONTROL, VK_V,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextW,
    };

    pub fn inject(text: &str, mode: OutputMode) -> Result<InjectOutcome> {
        match mode {
            OutputMode::Type => {
                send_unicode(text)?;
                Ok(InjectOutcome::Injected)
            }
            OutputMode::Paste => {
                paste_via_clipboard(text, send_ctrl_v)?;
                Ok(InjectOutcome::Injected)
            }
        }
    }

    fn key_input(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    /// Types arbitrary Unicode text, including UTF-16 surrogate pairs and
    /// newlines (sent as Return keystrokes so editors treat them as breaks).
    fn send_unicode(text: &str) -> Result<()> {
        use windows::Win32::UI::Input::KeyboardAndMouse::VK_RETURN;
        let mut inputs: Vec<INPUT> = Vec::with_capacity(text.len() * 2);
        for ch in text.chars() {
            if ch == '\n' {
                inputs.push(key_input(VK_RETURN, 0, KEYBD_EVENT_FLAGS(0)));
                inputs.push(key_input(VK_RETURN, 0, KEYEVENTF_KEYUP));
                continue;
            }
            let mut units = [0u16; 2];
            for unit in ch.encode_utf16(&mut units) {
                inputs.push(key_input(VIRTUAL_KEY(0), *unit, KEYEVENTF_UNICODE));
                inputs.push(key_input(
                    VIRTUAL_KEY(0),
                    *unit,
                    KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                ));
            }
        }
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize != inputs.len() {
            return Err(anyhow::anyhow!("SendInput sent {sent}/{}", inputs.len()));
        }
        Ok(())
    }

    fn send_ctrl_v() -> Result<()> {
        let inputs = [
            key_input(VK_CONTROL, 0, KEYBD_EVENT_FLAGS(0)),
            key_input(VK_V, 0, KEYBD_EVENT_FLAGS(0)),
            key_input(VK_V, 0, KEYEVENTF_KEYUP),
            key_input(VK_CONTROL, 0, KEYEVENTF_KEYUP),
        ];
        unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        Ok(())
    }

    /// Best-effort: UI Automation focused element IsPassword property.
    pub fn is_secure_field_focused() -> bool {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let Ok(uia): windows::core::Result<IUIAutomation> =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
            else {
                return false;
            };
            let Ok(elem) = uia.GetFocusedElement() else {
                return false;
            };
            elem.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false)
        }
    }

    pub fn frontmost_app_name() -> String {
        unsafe {
            let hwnd: HWND = GetForegroundWindow();
            if hwnd.0.is_null() {
                return String::new();
            }
            let mut buf = [0u16; 256];
            let len = GetWindowTextW(hwnd, &mut buf);
            String::from_utf16_lossy(&buf[..len as usize])
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use super::*;

    /// Linux is a development platform only: put the text on the clipboard
    /// so the developer can paste manually.
    pub fn inject(text: &str, _mode: OutputMode) -> Result<InjectOutcome> {
        let mut cb = arboard::Clipboard::new()?;
        cb.set_text(text.to_string())?;
        Ok(InjectOutcome::ClipboardOnly)
    }

    pub fn is_secure_field_focused() -> bool {
        false
    }

    pub fn frontmost_app_name() -> String {
        String::new()
    }
}
