use indexmap::IndexMap;
use serde::Serialize;
use serde_json::Value;
use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr;
use std::str::FromStr;

use crate::{Builder, Event, MpvFormat, MpvHandle, PropertyValue, Result};

#[derive(Serialize)]
struct FfiResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl FfiResponse {
    fn success(data: Value) -> Self {
        FfiResponse {
            data: Some(data),
            error: None,
        }
    }

    fn success_null() -> Self {
        FfiResponse {
            data: Some(Value::Null),
            error: None,
        }
    }

    fn error(msg: &str) -> Self {
        FfiResponse {
            data: None,
            error: Some(msg.to_string()),
        }
    }

    fn into_raw_json(self) -> *mut c_char {
        let json_string = match serde_json::to_string(&self) {
            Ok(s) => s,
            Err(e) => format!(r#"{{"error":"FFI: Failed to serialize response: {}"}}"#, e),
        };

        match CString::new(json_string) {
            Ok(c_str) => c_str.into_raw(),
            Err(_) => {
                let fallback = CString::new(
                    r#"{"error":"FFI: Invalid CString, potential null byte in response"}"#,
                )
                .unwrap();
                fallback.into_raw()
            }
        }
    }
}

/// Callback function type for mpv events.
///
/// @param event A JSON string representing the event.
/// @param userdata The user-supplied pointer passed to `mpv_wrapper_create`.
pub type EventCallback = unsafe extern "C" fn(event: *const c_char, userdata: *mut c_void);

/// Creates a new mpv handle.
///
/// @param initial_options A JSON string of initial mpv options (e.g., `{"idle": "yes"}`).
/// @param observed_properties A JSON string mapping property names to their formats (e.g., `{"pause": "flag"}`).
///                            The format can be "string", "flag", "int64", "double", or "node".
/// @param event_callback A function pointer that will be called for mpv events.
/// @param event_userdata A user-supplied pointer that will be passed to the event_callback.
/// @return A pointer to the opaque mpv handle, or NULL on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mpv_wrapper_create(
    initial_options: *const c_char,
    observed_properties: *const c_char,
    event_callback: EventCallback,
    event_userdata: *mut c_void,
) -> *mut MpvHandle {
    let initial_options_str = if initial_options.is_null() {
        "{}"
    } else {
        match unsafe { CStr::from_ptr(initial_options) }.to_str() {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "[libmpv-wrapper] Error: initial_options not valid UTF-8: {}",
                    e
                );
                return ptr::null_mut();
            }
        }
    };

    let observed_properties_str = if observed_properties.is_null() {
        "{}"
    } else {
        match unsafe { CStr::from_ptr(observed_properties) }.to_str() {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "[libmpv-wrapper] Error: observed_properties not valid UTF-8: {}",
                    e
                );
                return ptr::null_mut();
            }
        }
    };

    let initial_options: IndexMap<String, serde_json::Value> =
        match serde_json::from_str(initial_options_str) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "[libmpv-wrapper] Error: Failed to parse initial_options JSON: {}",
                    e
                );
                return ptr::null_mut();
            }
        };

    let observed_properties: IndexMap<String, MpvFormat> =
        match serde_json::from_str(observed_properties_str) {
            Ok(v) => v,
            Err(e) => {
                eprintln!(
                    "[libmpv-wrapper] Error: Failed to parse observed_properties JSON: {}",
                    e
                );
                return ptr::null_mut();
            }
        };

    let event_userdata_usize = event_userdata as usize;
    let event_handler = move |event: Event| -> Result<()> {
        let event_string = match serde_json::to_string(&event) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "[libmpv-wrapper] Event Error: Failed to serialize event: {}",
                    e
                );
                return Ok(());
            }
        };

        match CString::new(event_string) {
            Ok(c_event) => {
                let event_userdata_ptr = event_userdata_usize as *mut c_void;
                unsafe { event_callback(c_event.into_raw(), event_userdata_ptr) };
            }
            Err(e) => {
                eprintln!(
                    "[libmpv-wrapper] Event Error: Failed to create CString (null byte?): {}",
                    e
                );
            }
        }
        Ok(())
    };

    let builder = match Builder::new() {
        Ok(b) => b,
        Err(e) => {
            eprintln!(
                "[libmpv-wrapper] Error: Failed to create mpv builder: {}",
                e
            );
            return ptr::null_mut();
        }
    };

    let result = builder
        .set_options(initial_options)
        .and_then(|b| b.observed_properties(observed_properties))
        .map(|b| b.on_event(Box::new(event_handler)))
        .and_then(|b| b.build());

    match result {
        Ok(handle) => Box::into_raw(Box::new(handle)),
        Err(e) => {
            eprintln!("[libmpv-wrapper] Error: Failed to build mpv handle: {}", e);
            ptr::null_mut()
        }
    }
}

/// Destroys the mpv handle and terminates the mpv core.
///
/// @param handle A valid pointer to the mpv handle (obtained from `mpv_wrapper_create`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mpv_wrapper_destroy(handle: *mut MpvHandle) {
    if !handle.is_null() {
        let _ = unsafe { Box::from_raw(handle) };
    }
}

/// Executes an mpv command.
///
/// @param handle A valid pointer to the mpv handle.
/// @param name The name of the command (e.g., "set", "loadfile").
/// @param args A JSON string representing an array of arguments (e.g., `["volume", "50"]`, `["path/to/video.mp4"]`).
///             Pass an empty string "[]" or NULL for no arguments.
/// @return A JSON string representing the command result (e.g., `{"data": null}` or `{"error": "..."}`).
///         The caller MUST free this string using `mpv_wrapper_free_string`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mpv_wrapper_command(
    handle: *mut MpvHandle,
    name: *const c_char,
    args: *const c_char,
) -> *mut c_char {
    unsafe {
        if handle.is_null() || name.is_null() {
            return FfiResponse::error("Null pointer passed for mpv or name").into_raw_json();
        }

        let mpv_handle = &(*handle);

        let name_str = match CStr::from_ptr(name).to_str() {
            Ok(s) => s,
            Err(_) => {
                return FfiResponse::error("Invalid name string (not valid UTF-8)").into_raw_json();
            }
        };

        let args_strings: Vec<String> = if args.is_null() {
            Vec::new()
        } else {
            let args_str = match CStr::from_ptr(args).to_str() {
                Ok(s) => s,
                Err(_) => {
                    return FfiResponse::error("Invalid args_json string (not valid UTF-8)")
                        .into_raw_json();
                }
            };

            if args_str.is_empty() {
                Vec::new()
            } else {
                let args: Vec<serde_json::Value> = match serde_json::from_str(args_str) {
                    Ok(v) => v,
                    Err(e) => {
                        return FfiResponse::error(&format!(
                            "Failed to parse args_json (expected array): {}",
                            e
                        ))
                        .into_raw_json();
                    }
                };

                args.iter()
                    .map(|v| match v {
                        serde_json::Value::Bool(b) => b.to_string(),
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::String(s) => s.clone(),
                        _ => v.to_string().trim_matches('\"').to_string(),
                    })
                    .collect()
            }
        };

        let args_str_slice: Vec<&str> = args_strings.iter().map(|s| s.as_str()).collect();

        match mpv_handle.command(name_str, &args_str_slice) {
            Ok(_) => FfiResponse::success_null().into_raw_json(),
            Err(e) => FfiResponse::error(&format!("mpv command failed: {}", e)).into_raw_json(),
        }
    }
}

fn convert_serde_to_property(value: serde_json::Value) -> Option<PropertyValue> {
    match value {
        serde_json::Value::Bool(b) => Some(PropertyValue::Flag(b)),
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                Some(PropertyValue::Double(f))
            } else if let Some(i) = n.as_i64() {
                Some(PropertyValue::Int64(i))
            } else {
                None
            }
        }
        serde_json::Value::String(s) => Some(PropertyValue::String(s)),
        _ => None,
    }
}

/// Sets an mpv property.
///
/// @param handle A valid pointer to the mpv handle.
/// @param name The name of the property to set (e.g., "pause").
/// @param value A JSON string representing the value (e.g., "true").
/// @return A JSON string indicating success or failure.
///         The caller MUST free this string using `mpv_wrapper_free_string`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mpv_wrapper_set_property(
    handle: *mut MpvHandle,
    name: *const c_char,
    value: *const c_char,
) -> *mut c_char {
    unsafe {
        if handle.is_null() || name.is_null() || value.is_null() {
            return FfiResponse::error("Null pointer passed to set_property").into_raw_json();
        }

        let mpv_handle = &(*handle);
        let name_str = match CStr::from_ptr(name).to_str() {
            Ok(s) => s,
            Err(_) => {
                return FfiResponse::error("Invalid name string (not valid UTF-8)").into_raw_json();
            }
        };
        let value_str = match CStr::from_ptr(value).to_str() {
            Ok(s) => s,
            Err(_) => {
                return FfiResponse::error("Invalid value string (not valid UTF-8)")
                    .into_raw_json();
            }
        };

        let value: serde_json::Value = match serde_json::from_str(value_str) {
            Ok(v) => v,
            Err(_) => return FfiResponse::error("Failed to parse value").into_raw_json(),
        };

        let prop_value = match convert_serde_to_property(value) {
            Some(pv) => pv,
            None => {
                return FfiResponse::error("Unsupported value type for property").into_raw_json();
            }
        };

        match mpv_handle.set_property(name_str, prop_value) {
            Ok(_) => FfiResponse::success_null().into_raw_json(),
            Err(e) => FfiResponse::error(&format!("Failed to set property: {}", e)).into_raw_json(),
        }
    }
}

/// Gets an mpv property.
///
/// @param handle A valid pointer to the mpv handle.
/// @param name The name of the property to get.
/// @param format The format can be "string", "flag", "int64", "double", or "node".
/// @return A JSON string containing the property value (e.g., `{"data": true}`) or an error.
///         The caller MUST free this string using `mpv_wrapper_free_string`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mpv_wrapper_get_property(
    handle: *mut MpvHandle,
    name: *const c_char,
    format: *const c_char,
) -> *mut c_char {
    unsafe {
        if handle.is_null() || name.is_null() || format.is_null() {
            return FfiResponse::error("Null pointer passed to get_property").into_raw_json();
        }

        let mpv_handle = &(*handle);

        let name_str = match CStr::from_ptr(name).to_str() {
            Ok(s) => s,
            Err(_) => {
                return FfiResponse::error("Invalid name string (not valid UTF-8)").into_raw_json();
            }
        };

        let format_str = match CStr::from_ptr(format).to_str() {
            Ok(s) => s,
            Err(_) => {
                return FfiResponse::error("Invalid format string (not valid UTF-8)")
                    .into_raw_json();
            }
        };

        let mpv_format = MpvFormat::from_str(format_str).unwrap_or(MpvFormat::Node);

        match mpv_handle.get_property(name_str, mpv_format) {
            Ok(prop_value) => match serde_json::to_value(prop_value) {
                Ok(data_value) => FfiResponse::success(data_value).into_raw_json(),
                Err(e) => FfiResponse::error(&format!("Failed to serialize property value: {}", e))
                    .into_raw_json(),
            },
            Err(e) => FfiResponse::error(&format!("Failed to get property: {}", e)).into_raw_json(),
        }
    }
}

/// Frees a C string that was returned by one of the `mpv_wrapper_*` functions.
///
/// @param s A pointer to the C string to be freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mpv_wrapper_free(s: *mut c_char) {
    if !s.is_null() {
        let _ = unsafe { CString::from_raw(s) };
    }
}
