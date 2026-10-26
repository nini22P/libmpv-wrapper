use indexmap::IndexMap;
use libmpv_sys;
use scopeguard::defer;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::{convert::Infallible, ffi::CString};

use crate::get_lib;
use crate::{Error, MpvHandle, Result, utils::cstr_to_string, utils::error_string};

/// Reads a `mpv_format` field as the raw C integer. A Rust `mpv_format` may only hold one of its
/// declared values; a value mpv wrote that the bindings do not know (or garbage from a misread
/// struct) would be undefined behaviour to `match` on, so the raw code is matched instead.
unsafe fn raw_format(format: *const libmpv_sys::mpv_format) -> i32 {
    unsafe { *(format as *const i32) }
}

const FORMAT_NONE: i32 = libmpv_sys::mpv_format::MPV_FORMAT_NONE as i32;
const FORMAT_STRING: i32 = libmpv_sys::mpv_format::MPV_FORMAT_STRING as i32;
const FORMAT_OSD_STRING: i32 = libmpv_sys::mpv_format::MPV_FORMAT_OSD_STRING as i32;
const FORMAT_FLAG: i32 = libmpv_sys::mpv_format::MPV_FORMAT_FLAG as i32;
const FORMAT_INT64: i32 = libmpv_sys::mpv_format::MPV_FORMAT_INT64 as i32;
const FORMAT_DOUBLE: i32 = libmpv_sys::mpv_format::MPV_FORMAT_DOUBLE as i32;
const FORMAT_NODE: i32 = libmpv_sys::mpv_format::MPV_FORMAT_NODE as i32;
const FORMAT_NODE_ARRAY: i32 = libmpv_sys::mpv_format::MPV_FORMAT_NODE_ARRAY as i32;
const FORMAT_NODE_MAP: i32 = libmpv_sys::mpv_format::MPV_FORMAT_NODE_MAP as i32;
const FORMAT_BYTE_ARRAY: i32 = libmpv_sys::mpv_format::MPV_FORMAT_BYTE_ARRAY as i32;

fn format_to_string(format_code: i32) -> String {
    match format_code {
        FORMAT_NONE => "MPV_FORMAT_NONE".to_string(),
        FORMAT_STRING => "MPV_FORMAT_STRING".to_string(),
        FORMAT_OSD_STRING => "MPV_FORMAT_OSD_STRING".to_string(),
        FORMAT_FLAG => "MPV_FORMAT_FLAG".to_string(),
        FORMAT_INT64 => "MPV_FORMAT_INT64".to_string(),
        FORMAT_DOUBLE => "MPV_FORMAT_DOUBLE".to_string(),
        FORMAT_NODE => "MPV_FORMAT_NODE".to_string(),
        FORMAT_NODE_ARRAY => "MPV_FORMAT_NODE_ARRAY".to_string(),
        FORMAT_NODE_MAP => "MPV_FORMAT_NODE_MAP".to_string(),
        FORMAT_BYTE_ARRAY => "MPV_FORMAT_BYTE_ARRAY".to_string(),
        other => format!("unknown mpv_format {}", other),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MpvFormat {
    String,
    Flag,
    Int64,
    Double,
    Node,
}

impl From<MpvFormat> for libmpv_sys::mpv_format {
    fn from(format: MpvFormat) -> Self {
        match format {
            MpvFormat::String => libmpv_sys::mpv_format::MPV_FORMAT_STRING,
            MpvFormat::Flag => libmpv_sys::mpv_format::MPV_FORMAT_FLAG,
            MpvFormat::Int64 => libmpv_sys::mpv_format::MPV_FORMAT_INT64,
            MpvFormat::Double => libmpv_sys::mpv_format::MPV_FORMAT_DOUBLE,
            MpvFormat::Node => libmpv_sys::mpv_format::MPV_FORMAT_NODE,
        }
    }
}

impl FromStr for MpvFormat {
    type Err = Infallible;

    fn from_str(s: &str) -> std::result::Result<MpvFormat, Infallible> {
        match s {
            "string" => Ok(MpvFormat::String),
            "flag" => Ok(MpvFormat::Flag),
            "int64" => Ok(MpvFormat::Int64),
            "double" => Ok(MpvFormat::Double),
            "node" => Ok(MpvFormat::Node),
            _ => Ok(MpvFormat::String),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PropertyValue {
    String(String),
    Flag(bool),
    Int64(i64),
    Double(f64),
    Node(MpvNode),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MpvNode {
    None,
    String(String),
    Flag(bool),
    Int64(i64),
    Double(f64),
    NodeArray(Vec<MpvNode>),
    NodeMap(IndexMap<String, MpvNode>),
    ByteArray(Vec<u8>),
}

impl MpvNode {
    pub(crate) unsafe fn from_node(node: *const libmpv_sys::mpv_node) -> Result<Self> {
        match unsafe { raw_format(std::ptr::addr_of!((*node).format)) } {
            FORMAT_NONE => Ok(MpvNode::None),
            FORMAT_STRING => Ok(MpvNode::String(unsafe { cstr_to_string((*node).u.string) })),
            FORMAT_FLAG => Ok(MpvNode::Flag(unsafe { (*node).u.flag } != 0)),
            FORMAT_INT64 => Ok(MpvNode::Int64(unsafe { (*node).u.int64 })),
            FORMAT_DOUBLE => Ok(MpvNode::Double(unsafe { (*node).u.double_ })),
            FORMAT_NODE_ARRAY => {
                let list = unsafe { &*(*node).u.list };
                let mut vec = Vec::with_capacity(list.num as usize);
                for i in 0..list.num {
                    let child_node = unsafe { Self::from_node(list.values.add(i as usize)) }?;
                    vec.push(child_node);
                }
                Ok(MpvNode::NodeArray(vec))
            }
            FORMAT_NODE_MAP => {
                let list = unsafe { &*(*node).u.list };
                let mut map = IndexMap::with_capacity(list.num as usize);
                for i in 0..list.num {
                    let key = unsafe { cstr_to_string(*list.keys.add(i as usize)) };
                    let value_node = unsafe { Self::from_node(list.values.add(i as usize)) }?;
                    map.insert(key, value_node);
                }
                Ok(MpvNode::NodeMap(map))
            }
            FORMAT_BYTE_ARRAY => {
                let ba = unsafe { &*(*node).u.ba };
                let bytes =
                    unsafe { std::slice::from_raw_parts(ba.data as *const u8, ba.size).to_vec() };
                Ok(MpvNode::ByteArray(bytes))
            }
            format => Err(Error::NodeConversion(format!(
                "Unsupported mpv_node format: {}",
                format_to_string(format)
            ))),
        }
    }

    pub(crate) unsafe fn from_property(property: libmpv_sys::mpv_event_property) -> Result<Self> {
        match unsafe { raw_format(std::ptr::addr_of!(property.format)) } {
            FORMAT_NONE => Ok(MpvNode::None),
            FORMAT_STRING | FORMAT_OSD_STRING => {
                let str_ptr = unsafe { *(property.data as *const *const std::os::raw::c_char) };
                Ok(MpvNode::String(unsafe { cstr_to_string(str_ptr) }))
            }
            FORMAT_FLAG => Ok(MpvNode::Flag(
                unsafe { *(property.data as *const i32) } != 0,
            )),
            FORMAT_INT64 => Ok(MpvNode::Int64(unsafe { *(property.data as *const i64) })),
            FORMAT_DOUBLE => Ok(MpvNode::Double(unsafe { *(property.data as *const f64) })),
            FORMAT_NODE => unsafe {
                Self::from_node(property.data as *const libmpv_sys::mpv_node)
            },
            format => Err(Error::PropertyConversion(format!(
                "Unsupported mpv_event_property format: {}",
                format_to_string(format)
            ))),
        }
    }
}

macro_rules! get_property_impl {
    ($fn_name:ident, $ret_type:ty, $mpv_format:expr, $data_type:ty, $converter:expr) => {
        pub fn $fn_name(&self, name: &str) -> Result<$ret_type> {
            let lib = get_lib()?;

            let c_name = CString::new(name)?;
            let mut data: $data_type = Default::default();

            let err = unsafe {
                lib.mpv_get_property(
                    self.inner(),
                    c_name.as_ptr(),
                    $mpv_format,
                    &mut data as *mut _ as *mut _,
                )
            };

            if err < 0 {
                return Err(Error::GetProperty {
                    name: name.to_string(),
                    message: error_string(err),
                });
            }

            Ok($converter(data))
        }
    };
}

macro_rules! get_property_ptr_impl {
    (
        $fn_name:ident,
        $ret_type:ty,
        $mpv_format:expr,
        $ptr_type:ty,
        $free_method:ident,
        $null_ret:expr,
        $converter:expr
    ) => {
        pub fn $fn_name(&self, name: &str) -> Result<$ret_type> {
            let lib = get_lib()?;

            let c_name = CString::new(name)?;
            let mut data: $ptr_type = std::ptr::null_mut();

            let err = unsafe {
                lib.mpv_get_property(
                    self.inner(),
                    c_name.as_ptr(),
                    $mpv_format,
                    &mut data as *mut _ as *mut _,
                )
            };

            defer! {
                if !data.is_null() {
                    unsafe { lib.$free_method(data as *mut _) };
                }
            }

            if err < 0 {
                return Err(Error::GetProperty {
                    name: name.to_string(),
                    message: error_string(err),
                });
            }

            if data.is_null() {
                return Ok($null_ret);
            }

            unsafe { $converter(data) }
        }
    };
}

impl MpvHandle {
    get_property_ptr_impl!(
        get_property_string,
        String,
        libmpv_sys::mpv_format::MPV_FORMAT_STRING,
        *mut std::os::raw::c_char,
        mpv_free,
        String::new(),
        |data| Ok(std::ffi::CStr::from_ptr(data)
            .to_string_lossy()
            .into_owned())
    );

    get_property_impl!(
        get_property_flag,
        bool,
        libmpv_sys::mpv_format::MPV_FORMAT_FLAG,
        std::os::raw::c_int,
        |d| d != 0
    );

    get_property_impl!(
        get_property_int64,
        i64,
        libmpv_sys::mpv_format::MPV_FORMAT_INT64,
        i64,
        |d| d
    );

    get_property_impl!(
        get_property_double,
        f64,
        libmpv_sys::mpv_format::MPV_FORMAT_DOUBLE,
        f64,
        |d| d
    );

    /// `MPV_FORMAT_NODE` is unlike the other formats: mpv writes a whole `mpv_node` (not a pointer
    /// to one) into the `data` argument, so the node lives on our stack and only its contents are
    /// mpv-allocated and freed with `mpv_free_node_contents`. Reading it through
    /// `get_property_ptr_impl!` (a `*mut mpv_node` that mpv overwrote with the node's union) made
    /// the union's first word, a `mpv_node_list` pointer for arrays and maps, be dereferenced as a
    /// node: the "format" read from it was arbitrary and the exhaustive match on it jumped out of
    /// its jump table (crash with 0xc0000005 on Windows as soon as a track-list or similar property
    /// was read).
    pub fn get_property_node(&self, name: &str) -> Result<MpvNode> {
        let lib = get_lib()?;

        let c_name = CString::new(name)?;
        // All-zero is a valid node: format MPV_FORMAT_NONE, nothing to free.
        let mut node: libmpv_sys::mpv_node = unsafe { std::mem::zeroed() };

        let err = unsafe {
            lib.mpv_get_property(
                self.inner(),
                c_name.as_ptr(),
                libmpv_sys::mpv_format::MPV_FORMAT_NODE,
                &mut node as *mut libmpv_sys::mpv_node as *mut _,
            )
        };

        if err < 0 {
            return Err(Error::GetProperty {
                name: name.to_string(),
                message: error_string(err),
            });
        }

        // The node is only filled (and only has to be freed) when the call succeeded.
        let node_ptr: *mut libmpv_sys::mpv_node = &mut node;
        defer! {
            unsafe { lib.mpv_free_node_contents(node_ptr) };
        }

        unsafe { MpvNode::from_node(node_ptr as *const _) }
    }

    pub fn set_property(&self, name: &str, value: PropertyValue) -> Result<()> {
        let lib = get_lib()?;

        let c_name = CString::new(name)?;

        let err = unsafe {
            match value {
                PropertyValue::String(s) => {
                    let c_value = CString::new(s)?;
                    lib.mpv_set_property_string(self.inner(), c_name.as_ptr(), c_value.as_ptr())
                }
                PropertyValue::Flag(b) => {
                    let mut val: std::os::raw::c_int = if b { 1 } else { 0 };
                    lib.mpv_set_property(
                        self.inner(),
                        c_name.as_ptr(),
                        libmpv_sys::mpv_format::MPV_FORMAT_FLAG,
                        &mut val as *mut _ as *mut _,
                    )
                }
                PropertyValue::Int64(mut i) => lib.mpv_set_property(
                    self.inner(),
                    c_name.as_ptr(),
                    libmpv_sys::mpv_format::MPV_FORMAT_INT64,
                    &mut i as *mut _ as *mut _,
                ),
                PropertyValue::Double(mut f) => lib.mpv_set_property(
                    self.inner(),
                    c_name.as_ptr(),
                    libmpv_sys::mpv_format::MPV_FORMAT_DOUBLE,
                    &mut f as *mut _ as *mut _,
                ),
                PropertyValue::Node(_) => {
                    return Err(Error::Unsupported(
                        "Setting a property with a Node value is not supported".to_string(),
                    ));
                }
            }
        };

        if err < 0 {
            return Err(Error::SetProperty {
                name: name.to_string(),
                message: error_string(err),
            });
        }

        Ok(())
    }

    pub fn get_property(&self, name: &str, format: MpvFormat) -> Result<PropertyValue> {
        match format {
            MpvFormat::String => self.get_property_string(name).map(PropertyValue::String),
            MpvFormat::Flag => self.get_property_flag(name).map(PropertyValue::Flag),
            MpvFormat::Int64 => self.get_property_int64(name).map(PropertyValue::Int64),
            MpvFormat::Double => self.get_property_double(name).map(PropertyValue::Double),
            MpvFormat::Node => self.get_property_node(name).map(PropertyValue::Node),
        }
    }
}
