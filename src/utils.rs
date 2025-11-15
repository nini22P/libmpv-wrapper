use libmpv_sys;
use std::ffi::CStr;

pub fn error_string(err: i32) -> String {
    unsafe {
        let c_str = libmpv_sys::mpv_error_string(err);
        if c_str.is_null() {
            "Unknown error".to_string()
        } else {
            CStr::from_ptr(c_str).to_string_lossy().into_owned()
        }
    }
}

pub unsafe fn cstr_to_string(ptr: *const std::os::raw::c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(ptr).to_string_lossy().into_owned() }
}
