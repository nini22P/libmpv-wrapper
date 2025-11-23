mod builder;
mod command;
mod error;
mod event;
mod ffi;
mod property;
mod utils;

use std::sync::OnceLock;

use libmpv_sys::{self, Libmpv};

use crate::utils::get_current_dir;

pub use self::{
    builder::Builder,
    error::{Error, Result},
    event::Event,
    property::{MpvFormat, MpvNode, PropertyValue},
    utils::error_string,
};

static LIB: OnceLock<Result<Libmpv>> = OnceLock::new();

pub fn get_lib() -> Result<&'static Libmpv> {
    let result = LIB.get_or_init(|| unsafe { load() });

    match result {
        Ok(lib) => Ok(lib),
        Err(e) => Err(Error::Libmpv(format!(
            "libmpv library load failed: {:?}",
            e
        ))),
    }
}

unsafe fn load() -> Result<Libmpv> {
    #[cfg(target_os = "windows")]
    let lib_name = "libmpv-2.dll";
    #[cfg(target_os = "macos")]
    let lib_name = "libmpv.dylib";
    #[cfg(target_os = "linux")]
    let lib_name = "libmpv.so.2";

    if let Some(dir) = get_current_dir() {
        let lib_path = dir.join(lib_name);

        if lib_path.exists() {
            if let Ok(lib) = unsafe { Libmpv::new(&lib_path) } {
                return Ok(lib);
            }
        }
    }

    let lib = unsafe { Libmpv::new(lib_name) }?;

    Ok(lib)
}

pub struct MpvHandle(*mut libmpv_sys::mpv_handle);

impl MpvHandle {
    pub(crate) fn inner(&self) -> *mut libmpv_sys::mpv_handle {
        self.0
    }
}

unsafe impl Send for MpvHandle {}
unsafe impl Sync for MpvHandle {}

impl Drop for MpvHandle {
    fn drop(&mut self) {
        if !self.inner().is_null() {
            if let Ok(lib) = get_lib() {
                unsafe {
                    lib.mpv_terminate_destroy(self.inner());
                }
            }
            self.0 = std::ptr::null_mut();
        }
    }
}
