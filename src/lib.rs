mod builder;
mod command;
mod error;
mod event;
mod ffi;
mod property;
mod utils;

use libmpv_sys;

pub use self::{
    builder::Builder,
    error::{Error, Result},
    event::Event,
    property::{MpvFormat, MpvNode, PropertyValue},
    utils::error_string,
};

#[derive(Debug)]
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
            unsafe {
                libmpv_sys::mpv_terminate_destroy(self.inner());
            }
            self.0 = std::ptr::null_mut();
        }
    }
}
