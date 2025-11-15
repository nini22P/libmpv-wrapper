use libloading::{Library, Symbol};
use std::env;
use std::ffi::{CStr, CString, c_char, c_void};
use std::path::PathBuf;
use std::sync::mpsc::{Sender, channel};
use std::time::{Duration, Instant};

pub type EventCallback = unsafe extern "C" fn(event: *const c_char, userdata: *mut c_void);

type FnCreate = unsafe extern "C" fn(
    initial_options: *const c_char,
    observed_properties: *const c_char,
    event_callback: EventCallback,
    event_userdata: *mut c_void,
) -> *mut c_void;

type FnDestroy = unsafe extern "C" fn(mpv: *mut c_void);

type FnCommand =
    unsafe extern "C" fn(mpv: *mut c_void, name: *const c_char, args: *const c_char) -> *mut c_char;

type FnSetProperty = unsafe extern "C" fn(
    mpv: *mut c_void,
    name: *const c_char,
    value: *const c_char,
) -> *mut c_char;

type FnGetProperty = unsafe extern "C" fn(
    mpv: *mut c_void,
    name: *const c_char,
    format: *const c_char,
) -> *mut c_char;
type FnFreeString = unsafe extern "C" fn(s: *mut c_char);

unsafe extern "C" fn event_callback(event: *const c_char, userdata: *mut c_void) {
    unsafe {
        if event.is_null() {
            return;
        }

        let event_str = CStr::from_ptr(event).to_string_lossy();

        let tx = &*(userdata as *const Sender<serde_json::Value>);

        if let Ok(event_json) = serde_json::from_str::<serde_json::Value>(&event_str) {
            let _ = tx.send(event_json);
        }
    }
}

fn find_library_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let mut lib_path = env::current_exe()?;
    lib_path.pop();

    #[cfg(target_os = "windows")]
    let lib_name = "libmpv_wrapper.dll";
    #[cfg(target_os = "macos")]
    let lib_name = "libmpv_wrapper.dylib";
    #[cfg(target_os = "linux")]
    let lib_name = "libmpv_wrapper.so";

    lib_path.push(lib_name);
    if lib_path.exists() {
        return Ok(lib_path);
    }

    Err(format!("Could not find library {} in {:?}", lib_name, lib_path).into())
}

#[test]
fn test_ffi() -> Result<(), Box<dyn std::error::Error>> {
    println!("Starting FFI test...");

    std::thread::sleep(Duration::from_millis(500));

    let lib_path = find_library_path()?;
    println!("Loading library from: {:?}", lib_path);

    unsafe {
        let lib = Library::new(&lib_path)?;

        let lib: &'static Library = Box::leak(Box::new(lib));

        let mpv_create: Symbol<'static, FnCreate> = lib.get(b"mpv_wrapper_create")?;
        let mpv_destroy: Symbol<'static, FnDestroy> = lib.get(b"mpv_wrapper_destroy")?;
        let mpv_command: Symbol<'static, FnCommand> = lib.get(b"mpv_wrapper_command")?;
        let mpv_set_property: Symbol<'static, FnSetProperty> =
            lib.get(b"mpv_wrapper_set_property")?;
        let mpv_get_property: Symbol<'static, FnGetProperty> =
            lib.get(b"mpv_wrapper_get_property")?;
        let mpv_free_string: Symbol<'static, FnFreeString> = lib.get(b"mpv_wrapper_free_string")?;

        let (tx, rx) = channel::<serde_json::Value>();
        let event_userdata = &tx as *const _ as *mut c_void;

        let c_initial_options = CString::new(r#"{"idle": "yes", "vo": "null"}"#)?;
        let c_observed_properties =
            CString::new(r#"{"pause": "flag", "volume": "double", "time-pos": "double"}"#)?;

        println!("Creating mpv...");
        let mpv = mpv_create(
            c_initial_options.as_ptr(),
            c_observed_properties.as_ptr(),
            event_callback,
            event_userdata,
        );
        assert!(!mpv.is_null(), "Failed to create mpv");

        println!("Waiting for initial events...");
        let start = Instant::now();
        let mut events_received = 0;

        while start.elapsed() < Duration::from_secs(5) && events_received < 2 {
            if let Ok(event) = rx.recv_timeout(Duration::from_millis(200)) {
                if event["event"] == "property-change" {
                    println!("...initial event: {:?}", event);
                    events_received += 1;
                }
            }
        }
        println!("Initial events received: {}", events_received);
        assert!(
            events_received > 0,
            "Did not receive initial property-change events"
        );

        println!("Testing command 'set', ['volume' '50']...");
        let c_name = CString::new("set")?;
        let c_args = CString::new(r#"["volume", "50"]"#)?;

        let result_ptr = mpv_command(mpv, c_name.as_ptr(), c_args.as_ptr());
        assert!(!result_ptr.is_null(), "mpv_command returned null");

        let result_str = CStr::from_ptr(result_ptr).to_string_lossy();
        println!("Command response: {}", result_str);
        let result_val: serde_json::Value = serde_json::from_str(&result_str)?;

        assert!(
            result_val["error"].is_null(),
            "mpv_command returned an error: {}",
            result_val["error"]
        );

        mpv_free_string(result_ptr);

        println!("Waiting for 'volume' = 50 event...");
        let start_cmd = Instant::now();
        loop {
            let event = rx.recv_timeout(Duration::from_secs(5))?;

            if event["name"] == "volume" {
                if let Some(data) = event.get("data") {
                    if let Some(vol) = data.as_f64() {
                        if (vol - 50.0).abs() < 0.1 {
                            println!("Verified command via property change: {:?}", event);
                            break;
                        }
                    }
                }
            }
            println!("...skipping irrelevant event: {:?}", event);
            if start_cmd.elapsed() > Duration::from_secs(5) {
                panic!(
                    "Timeout: Never received 'volume: 50' event. Last event: {:?}",
                    event
                );
            }
        }

        println!("Setting property 'pause' = true...");
        let c_name = CString::new("pause")?;
        let c_value = CString::new("true")?;

        let result_ptr = mpv_set_property(mpv, c_name.as_ptr(), c_value.as_ptr());
        assert!(!result_ptr.is_null(), "mpv_set_property returned null");

        let result_str = CStr::from_ptr(result_ptr).to_string_lossy();
        println!("Set property response: {}", result_str);
        let result_val: serde_json::Value = serde_json::from_str(&result_str)?;

        assert!(
            result_val["error"].is_null(),
            "mpv_set_property returned an error: {}",
            result_val["error"]
        );

        mpv_free_string(result_ptr);

        println!("Waiting for 'pause' = true event...");
        let start_set = Instant::now();
        loop {
            let event = rx.recv_timeout(Duration::from_secs(5))?;

            if event["event"] == "property-change"
                && event["name"] == "pause"
                && event["data"] == serde_json::Value::Bool(true)
            {
                println!("Verified set_property: {:?}", event);
                assert_eq!(event["name"], "pause");
                assert_eq!(event["data"], serde_json::Value::Bool(true));
                break;
            }

            println!("...skipping irrelevant event: {:?}", event);
            if start_set.elapsed() > Duration::from_secs(5) {
                panic!(
                    "Timeout: Never received 'pause: true' event. Last event received: {:?}",
                    event
                );
            }
        }

        println!("Getting property 'pause'...");
        let c_name = CString::new("pause")?;
        let c_format = CString::new("flag")?;
        let result_ptr = mpv_get_property(mpv, c_name.as_ptr(), c_format.as_ptr());
        assert!(!result_ptr.is_null(), "get_property returned null");

        let result_str = CStr::from_ptr(result_ptr).to_string_lossy();
        println!("Got property string: {}", result_str);

        let result_val: serde_json::Value = serde_json::from_str(&result_str)?;
        assert!(
            result_val["error"].is_null(),
            "get_property returned an error: {}",
            result_val["error"]
        );

        let data_val = &result_val["data"];
        assert_eq!(data_val, &serde_json::Value::Bool(true));
        println!("Verified 'pause' property via get_property is true.");

        mpv_free_string(result_ptr);

        println!("Destroying mpv...");
        mpv_destroy(mpv);

        println!("Waiting for 'shutdown' event...");
        let start_shutdown = Instant::now();
        loop {
            let event = rx.recv_timeout(Duration::from_secs(5))?;
            println!("...checking for shutdown: {:?}", event);

            if event["event"] == "shutdown" {
                println!("Verified shutdown.");
                assert_eq!(event["event"], "shutdown");
                break;
            }

            if start_shutdown.elapsed() > Duration::from_secs(7) {
                panic!(
                    "Timeout: Never received 'shutdown' event. Last event received: {:?}",
                    event
                );
            }
        }

        println!("Test finished successfully!");
    }

    Ok(())
}
