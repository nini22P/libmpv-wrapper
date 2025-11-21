use std::ffi::CStr;

use crate::get_lib;

pub fn error_string(err: i32) -> String {
    match get_lib() {
        Ok(lib) => unsafe {
            let c_str = lib.mpv_error_string(err);

            if c_str.is_null() {
                "Unknown error".to_string()
            } else {
                CStr::from_ptr(c_str).to_string_lossy().into_owned()
            }
        },
        Err(_) => {
            format!("Mpv error code {} (Library not loaded)", err)
        }
    }
}

pub unsafe fn cstr_to_string(ptr: *const std::os::raw::c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr).to_string_lossy().into_owned() }
}
