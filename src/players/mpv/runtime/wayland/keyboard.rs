use std::ffi::CString;
use std::fs::File;
use std::io::Read;
use std::os::fd::OwnedFd;
use std::ptr;

use xkbcommon_dl::{
    XKB_MOD_NAME_ALT, XKB_MOD_NAME_CAPS, XKB_MOD_NAME_CTRL, XKB_MOD_NAME_LOGO, XKB_MOD_NAME_SHIFT,
    XkbCommon, xkb_context, xkb_context_flags, xkb_key_direction, xkb_keymap,
    xkb_keymap_compile_flags, xkb_keymap_format, xkb_state, xkb_state_component, xkbcommon_option,
};

use super::WaylandModifiers;

pub(super) struct WaylandKeymap {
    xkb: &'static XkbCommon,
    ctx: *mut xkb_context,
    keymap: *mut xkb_keymap,
    state: *mut xkb_state,
}

impl WaylandKeymap {
    pub fn new() -> Option<Self> {
        let xkb = xkbcommon_option()?;
        // SAFETY: `xkb_context_new` takes only flags and returns an owned context pointer or null.
        let ctx = unsafe { (xkb.xkb_context_new)(xkb_context_flags::XKB_CONTEXT_NO_FLAGS) };
        if ctx.is_null() {
            return None;
        }
        Some(Self {
            xkb,
            ctx,
            keymap: ptr::null_mut(),
            state: ptr::null_mut(),
        })
    }

    pub fn load_fd(&mut self, fd: OwnedFd, size: u32) {
        let len = (size as usize).min(16 * 1024 * 1024);
        let mut buf = vec![0_u8; len];
        let mut file = File::from(fd);
        if file.read_exact(&mut buf).is_err() {
            return;
        }
        if buf.last() != Some(&0) {
            buf.push(0);
        }
        let Ok(c_keymap) = CString::from_vec_with_nul(buf).or_else(|e| {
            let mut trimmed = e.into_bytes();
            trimmed.truncate(
                trimmed
                    .iter()
                    .position(|&b| b == 0)
                    .unwrap_or(trimmed.len()),
            );
            CString::new(trimmed)
        }) else {
            return;
        };
        // SAFETY: `ctx` is live and `c_keymap` is NUL-terminated UTF-8 XKB rules.
        let next_keymap = unsafe {
            (self.xkb.xkb_keymap_new_from_string)(
                self.ctx,
                c_keymap.as_ptr(),
                xkb_keymap_format::XKB_KEYMAP_FORMAT_TEXT_V1,
                xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS,
            )
        };
        if next_keymap.is_null() {
            return;
        }
        // SAFETY: `next_keymap` is non-null and owned by us until unref.
        let next_state = unsafe { (self.xkb.xkb_state_new)(next_keymap) };
        if next_state.is_null() {
            // SAFETY: `next_keymap` was newly created above.
            unsafe { (self.xkb.xkb_keymap_unref)(next_keymap) };
            return;
        }
        self.clear_keymap();
        self.keymap = next_keymap;
        self.state = next_state;
    }

    pub fn update_mask(
        &mut self,
        mods_depressed: u32,
        mods_latched: u32,
        mods_locked: u32,
        group: u32,
    ) {
        if self.state.is_null() {
            return;
        }
        // SAFETY: `self.state` is a valid non-null `xkb_state`.
        unsafe {
            (self.xkb.xkb_state_update_mask)(
                self.state,
                mods_depressed,
                mods_latched,
                mods_locked,
                0,
                0,
                group,
            );
        }
    }

    pub fn modifiers(&self, existing: WaylandModifiers) -> WaylandModifiers {
        if self.state.is_null() {
            return WaylandModifiers {
                left_button: existing.left_button,
                middle_button: existing.middle_button,
                right_button: existing.right_button,
                ..WaylandModifiers::default()
            };
        }
        let active = |name: &[u8]| -> bool {
            // SAFETY: `self.state` is valid and all `XKB_MOD_NAME_*` slices end with `\0`.
            unsafe {
                (self.xkb.xkb_state_mod_name_is_active)(
                    self.state,
                    name.as_ptr().cast(),
                    xkb_state_component::XKB_STATE_MODS_EFFECTIVE,
                ) > 0
            }
        };
        WaylandModifiers {
            shift: active(XKB_MOD_NAME_SHIFT),
            caps_lock: active(XKB_MOD_NAME_CAPS),
            control: active(XKB_MOD_NAME_CTRL),
            alt: active(XKB_MOD_NAME_ALT),
            super_key: active(XKB_MOD_NAME_LOGO),
            left_button: existing.left_button,
            middle_button: existing.middle_button,
            right_button: existing.right_button,
        }
    }

    pub fn translate_key(&mut self, evdev_code: u32, pressed: bool) -> (u32, u32) {
        if self.state.is_null() || self.keymap.is_null() {
            return (0, 0);
        }
        let xkb_code = evdev_code.saturating_add(8);
        // SAFETY: `self.state` and `self.keymap` are valid non-null pointers.
        let sym = unsafe { (self.xkb.xkb_state_key_get_one_sym)(self.state, xkb_code) };
        let mut syms_ptr: *const u32 = ptr::null();
        // SAFETY: `self.keymap` is valid and `syms_ptr` receives an internal slice pointer.
        let count = unsafe {
            (self.xkb.xkb_keymap_key_get_syms_by_level)(
                self.keymap,
                xkb_code,
                0,
                0,
                &raw mut syms_ptr,
            )
        };
        let base_sym = if count > 0 && !syms_ptr.is_null() {
            // SAFETY: `xkb_keymap_key_get_syms_by_level` returned `count > 0` for `syms_ptr`.
            unsafe { *syms_ptr }
        } else {
            sym
        };
        let dir = if pressed {
            xkb_key_direction::XKB_KEY_DOWN
        } else {
            xkb_key_direction::XKB_KEY_UP
        };
        // SAFETY: `self.state` is a valid non-null `xkb_state`.
        unsafe { (self.xkb.xkb_state_update_key)(self.state, xkb_code, dir) };
        (base_sym, sym)
    }

    fn clear_keymap(&mut self) {
        if !self.state.is_null() {
            // SAFETY: `self.state` was allocated by `xkb_state_new`.
            unsafe { (self.xkb.xkb_state_unref)(self.state) };
            self.state = ptr::null_mut();
        }
        if !self.keymap.is_null() {
            // SAFETY: `self.keymap` was allocated by `xkb_keymap_new_from_string`.
            unsafe { (self.xkb.xkb_keymap_unref)(self.keymap) };
            self.keymap = ptr::null_mut();
        }
    }
}

impl Drop for WaylandKeymap {
    fn drop(&mut self) {
        self.clear_keymap();
        if !self.ctx.is_null() {
            // SAFETY: `self.ctx` was allocated by `xkb_context_new`.
            unsafe { (self.xkb.xkb_context_unref)(self.ctx) };
            self.ctx = ptr::null_mut();
        }
    }
}
