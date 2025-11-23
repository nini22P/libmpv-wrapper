use std::{ffi::CStr, path::PathBuf};

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

pub fn get_current_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        use std::ffi::c_void;
        use std::ptr;
        unsafe extern "system" {
            fn GetModuleHandleExW(
                dwFlags: u32,
                lpModuleName: *const u16,
                phModule: *mut *mut c_void,
            ) -> i32;
            fn GetModuleFileNameW(hModule: *mut c_void, lpFilename: *mut u16, nSize: u32) -> u32;
        }

        const GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS: u32 = 0x00000004;
        const GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT: u32 = 0x00000002;

        unsafe {
            let mut h_module: *mut c_void = ptr::null_mut();
            let func_addr = get_current_dir as *const c_void;

            let result = GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                func_addr as *const u16,
                &mut h_module,
            );

            if result == 0 || h_module.is_null() {
                return None;
            }

            let mut buffer = [0u16; 1024];
            let len = GetModuleFileNameW(h_module, buffer.as_mut_ptr(), 1024);

            if len == 0 {
                return None;
            }

            let path = PathBuf::from(String::from_utf16_lossy(&buffer[..len as usize]));
            path.parent().map(|p| p.to_path_buf())
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::ffi::{CStr, c_void};
        use std::os::unix::ffi::OsStrExt;

        unsafe extern "C" {
            fn dladdr(addr: *const c_void, info: *mut DlInfo) -> i32;
        }

        #[repr(C)]
        struct DlInfo {
            dli_fname: *const i8,
            dli_fbase: *mut c_void,
            dli_sname: *const i8,
            dli_saddr: *mut c_void,
        }

        unsafe {
            let mut info: DlInfo = std::mem::zeroed();
            let func_addr = get_current_dir as *const c_void;

            if dladdr(func_addr, &mut info) != 0 && !info.dli_fname.is_null() {
                let fname = CStr::from_ptr(info.dli_fname);
                let path = PathBuf::from(std::ffi::OsStr::from_bytes(fname.to_bytes()));
                return path.parent().map(|p| p.to_path_buf());
            }
        }
        None
    }
}
