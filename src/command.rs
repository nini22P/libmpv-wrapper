use libmpv_sys;
use std::ffi::CString;

use crate::{Error, MpvHandle, Result, utils::error_string};

impl MpvHandle {
    pub fn command(&self, name: &str, args: &[&str]) -> Result<()> {
        let c_args: Vec<CString> = std::iter::once(name)
            .chain(args.iter().cloned())
            .map(CString::new)
            .map(|res| res.map_err(|e| Error::InvalidParameter(e.to_string())))
            .collect::<Result<Vec<_>>>()?;

        let mut c_pointers: Vec<*const std::os::raw::c_char> =
            c_args.iter().map(|s| s.as_ptr()).collect();
        c_pointers.push(std::ptr::null());

        let err = unsafe { libmpv_sys::mpv_command(self.inner(), c_pointers.as_mut_ptr()) };

        if err < 0 {
            return Err(Error::Command {
                name: name.to_string(),
                message: error_string(err),
            });
        }

        Ok(())
    }
}
