use indexmap::IndexMap;
use log::info;
use std::ffi::CString;

use crate::{
    Error, Event, MpvFormat, MpvHandle, Result, error_string,
    event::{EventHandler, EventListener, start_event_listener},
    get_lib,
};

pub struct Builder {
    handle: MpvHandle,
    event_handle: MpvHandle,
    event_handler: Option<EventHandler>,
}

impl Builder {
    pub fn new() -> Result<Self> {
        let lib = get_lib()?;

        let handle = unsafe { lib.mpv_create() };
        if handle.is_null() {
            return Err(Error::Create);
        }

        let event_handle = unsafe {
            lib.mpv_create_client(handle, CString::new("event-client").unwrap().as_ptr())
        };

        if event_handle.is_null() {
            unsafe { lib.mpv_terminate_destroy(handle) };
            return Err(Error::ClientCreation);
        }

        Ok(Self {
            handle: MpvHandle(handle),
            event_handle: MpvHandle(event_handle),
            event_handler: None,
        })
    }

    pub fn set_options(self, options: IndexMap<String, serde_json::Value>) -> Result<Self> {
        let lib = get_lib()?;

        for (name, value) in options {
            let value_str = match value {
                serde_json::Value::Bool(b) => if b { "yes" } else { "no" }.to_string(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::String(s) => s,
                _ => continue,
            };

            if name.is_empty() {
                continue;
            }

            let c_name = CString::new(name.clone())?;
            let c_value = CString::new(value_str)?;

            let err = unsafe {
                lib.mpv_set_option_string(self.handle.inner(), c_name.as_ptr(), c_value.as_ptr())
            };

            if err < 0 {
                return Err(Error::SetOption {
                    name,
                    message: error_string(err),
                });
            }
        }
        Ok(self)
    }

    pub fn observed_properties(self, properties: IndexMap<String, MpvFormat>) -> Result<Self> {
        let lib = get_lib()?;

        for (i, (name, format)) in properties.iter().enumerate() {
            let property_id = (i + 1) as u64;

            info!(
                "Observing property '{}' (ID: {}) with format '{:?}'",
                name, property_id, format
            );

            let c_name = CString::new(name.clone())?;

            let err = unsafe {
                lib.mpv_observe_property(
                    self.event_handle.inner(),
                    property_id,
                    c_name.as_ptr(),
                    (*format).into(),
                )
            };

            if err < 0 {
                return Err(Error::PropertyObserve {
                    name: name.to_string(),
                    message: error_string(err),
                });
            }
        }
        Ok(self)
    }

    pub fn on_event<F>(mut self, handler: F) -> Self
    where
        F: FnMut(Event) -> Result<()> + Send + 'static,
    {
        self.event_handler = Some(Box::new(handler));
        self
    }

    pub fn build(mut self) -> Result<MpvHandle> {
        let lib = get_lib()?;

        let err = unsafe { lib.mpv_initialize(self.handle.inner()) };
        if err < 0 {
            return Err(Error::Initialize(error_string(err)));
        }

        let event_handler = self.event_handler.take();

        let handle = std::mem::replace(&mut self.handle, MpvHandle(std::ptr::null_mut()));
        let event_handle =
            std::mem::replace(&mut self.event_handle, MpvHandle(std::ptr::null_mut()));

        if let Some(handler) = event_handler {
            let event_listener = EventListener { event_handle };
            start_event_listener(handler, event_listener);
        } else if !event_handle.inner().is_null() {
            unsafe { lib.mpv_destroy(event_handle.inner()) };
            std::mem::forget(event_handle);
        }

        std::mem::forget(self);

        Ok(handle)
    }
}

impl Drop for Builder {
    fn drop(&mut self) {
        if !self.handle.inner().is_null() {
            if let Ok(lib) = get_lib() {
                unsafe { lib.mpv_terminate_destroy(self.handle.inner()) };
            }
        }
    }
}
