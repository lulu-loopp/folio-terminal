//! Read-only IME evidence for one live application window on its event-loop thread.
//! No context association, COM initialization, profile activation, or focus changes.

use crate::NativeWindow;

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeFacts {
    pub hwnd: Option<usize>,
    pub focused_hwnd: Option<usize>,
    pub focus_matches: Option<bool>,
    pub context: Option<bool>,
    pub open: Option<bool>,
    pub conversion: Option<u32>,
    pub hkl_low: Option<u16>,
    pub hkl_high: Option<u16>,
    pub imm_is_ime: Option<bool>,
    pub tsf_profile_type: Option<u32>,
    pub tsf_error: Option<i32>,
}

impl NativeFacts {
    /// A language id alone never establishes that an input method is active.
    /// IMM's mode is a compatibility reading for TSF; unknown is not native.
    pub fn composing_mode(self) -> bool {
        (self.tsf_profile_type == Some(1) || self.imm_is_ime == Some(true))
            && self.context == Some(true)
            && self.open == Some(true)
            && self.conversion.is_some_and(|mode| mode & 1 != 0)
    }
}

#[cfg(not(target_os = "windows"))]
pub fn snapshot(_window: NativeWindow) -> NativeFacts {
    // These facts are not already read on macOS. Do not create an input context
    // or introduce a second AppKit path just to observe it.
    NativeFacts::default()
}

#[cfg(target_os = "windows")]
pub fn snapshot(window: NativeWindow) -> NativeFacts {
    use windows::Win32::{
        System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance},
        UI::{
            Input::{
                Ime::{
                    IME_CONVERSION_MODE, ImmGetContext, ImmGetConversionStatus, ImmGetOpenStatus,
                    ImmIsIME, ImmReleaseContext,
                },
                KeyboardAndMouse::{GetFocus, GetKeyboardLayout},
            },
            TextServices::{
                CLSID_TF_InputProcessorProfiles, GUID_TFCAT_TIP_KEYBOARD,
                ITfInputProcessorProfileMgr, TF_INPUTPROCESSORPROFILE,
            },
        },
    };

    let hwnd = window.as_hwnd();
    // SAFETY: the caller supplies its own live winit window on the event-loop
    // thread. GetFocus/GetKeyboardLayout read this thread only. Every acquired
    // HIMC is released against exactly that HWND, even if the mode query fails.
    unsafe {
        let focused = GetFocus();
        let hkl = GetKeyboardLayout(0);
        let context = ImmGetContext(hwnd);
        let mut facts = NativeFacts {
            hwnd: Some(hwnd.0 as usize),
            focused_hwnd: Some(focused.0 as usize),
            focus_matches: Some(hwnd == focused),
            context: Some(!context.0.is_null()),
            hkl_low: Some(hkl.0 as usize as u16),
            hkl_high: Some(((hkl.0 as usize) >> 16) as u16),
            imm_is_ime: Some(ImmIsIME(hkl).as_bool()),
            ..NativeFacts::default()
        };
        if !context.0.is_null() {
            facts.open = Some(ImmGetOpenStatus(context).as_bool());
            let mut conversion = IME_CONVERSION_MODE::default();
            if ImmGetConversionStatus(context, Some(&mut conversion), None).as_bool() {
                facts.conversion = Some(conversion.0);
            }
            let _ = ImmReleaseContext(hwnd, context);
        }
        // This is the local profile manager, not a TIP activation or a TSF text
        // store. Borrow the apartment already established by the application;
        // CO_E_NOTINITIALIZED is evidence, not a reason to initialize COM here.
        let profile = (|| -> windows::core::Result<u32> {
            let manager: ITfInputProcessorProfileMgr =
                CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)?;
            let mut profile = TF_INPUTPROCESSORPROFILE::default();
            manager.GetActiveProfile(&GUID_TFCAT_TIP_KEYBOARD, &mut profile)?;
            Ok(profile.dwProfileType)
        })();
        match profile {
            // S_FALSE leaves the zero-initialized type at zero: unknown, not a keyboard.
            Ok(kind @ (1 | 2)) => facts.tsf_profile_type = Some(kind),
            Ok(_) => {}
            Err(error) => facts.tsf_error = Some(error.code().0),
        }
        facts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ime_observation_unknown_is_not_native_mode() {
        assert!(!NativeFacts::default().composing_mode());
    }

    #[test]
    fn ime_observation_reads_only_the_supplied_window() {
        let source = include_str!("ime_observation.rs");
        let reader = source.split("#[cfg(test)]").next().unwrap();
        assert!(reader.contains("ImmGetContext(hwnd)"));
        assert!(reader.contains("ImmReleaseContext(hwnd, context)"));
        for mutation in [
            "ImmSet",
            "ImmAssociate",
            "SetFocus(",
            "CoInitialize",
            "ActivateProfile(",
        ] {
            assert!(!reader.contains(mutation), "unexpected mutation {mutation}");
        }
    }
}
