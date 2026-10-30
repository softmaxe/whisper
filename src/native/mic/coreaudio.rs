//! In-process CoreAudio queries for the input device list and the system
//! default input.

use std::ffi::c_void;

use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};

use super::MicDevice;
use crate::native::fourcc;

pub type AudioObjectId = u32;

#[repr(C)]
pub struct PropertyAddress {
    pub selector: u32,
    pub scope: u32,
    pub element: u32,
}

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectGetPropertyDataSize(
        object: AudioObjectId,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
    ) -> i32;
    fn AudioObjectGetPropertyData(
        object: AudioObjectId,
        address: *const PropertyAddress,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> i32;
}

const SYSTEM_OBJECT: AudioObjectId = 1;
const SCOPE_GLOBAL: u32 = fourcc(b"glob");
const SCOPE_INPUT: u32 = fourcc(b"inpt");
const ELEMENT_MAIN: u32 = 0;
const DEVICES: u32 = fourcc(b"dev#");
const DEFAULT_INPUT: u32 = fourcc(b"dIn ");
const STREAM_CONFIGURATION: u32 = fourcc(b"slay");
const NAME: u32 = fourcc(b"lnam");
const UID: u32 = fourcc(b"uid ");
const TRANSPORT_TYPE: u32 = fourcc(b"tran");
const TRANSPORT_BUILT_IN: u32 = fourcc(b"bltn");

fn address(selector: u32, scope: u32) -> PropertyAddress {
    PropertyAddress {
        selector,
        scope,
        element: ELEMENT_MAIN,
    }
}

fn data_size(object: AudioObjectId, address: &PropertyAddress) -> Option<u32> {
    let mut size = 0u32;
    // SAFETY: valid address and out-pointer.
    let status =
        unsafe { AudioObjectGetPropertyDataSize(object, address, 0, std::ptr::null(), &mut size) };
    (status == 0).then_some(size)
}

fn read_u32(object: AudioObjectId, selector: u32) -> Option<u32> {
    let address = address(selector, SCOPE_GLOBAL);
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: the property is a UInt32 and `value` has room for it.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            (&mut value as *mut u32).cast(),
        )
    };
    (status == 0).then_some(value)
}

fn read_string(object: AudioObjectId, selector: u32) -> Option<String> {
    let address = address(selector, SCOPE_GLOBAL);
    let mut value: CFStringRef = std::ptr::null();
    let mut size = std::mem::size_of::<CFStringRef>() as u32;
    // SAFETY: the property is a CFStringRef returned under the create rule.
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            (&mut value as *mut CFStringRef).cast(),
        )
    };
    if status != 0 || value.is_null() {
        return None;
    }
    // SAFETY: non-null CFString we own.
    Some(unsafe { CFString::wrap_under_create_rule(value) }.to_string())
}

fn input_channels(device: AudioObjectId) -> u32 {
    let address = address(STREAM_CONFIGURATION, SCOPE_INPUT);
    let Some(size) = data_size(device, &address).filter(|s| *s >= 8) else {
        return 0;
    };
    // AudioBufferList: UInt32 count, then {UInt32 channels, UInt32 bytes,
    // void *data} per buffer (16 bytes, 8-aligned on 64-bit).
    let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
    let mut size = size;
    // SAFETY: `buffer` holds `size` bytes, 8-aligned.
    let status = unsafe {
        AudioObjectGetPropertyData(
            device,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            buffer.as_mut_ptr().cast(),
        )
    };
    if status != 0 {
        return 0;
    }
    let bytes: &[u8] =
        // SAFETY: reinterpreting the initialized u64 buffer as bytes.
        unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast(), size as usize) };
    let read = |offset: usize| -> u32 {
        bytes
            .get(offset..offset + 4)
            .map(|b| u32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
            .unwrap_or(0)
    };
    let count = read(0) as usize;
    (0..count).map(|i| read(8 + i * 16)).sum()
}

pub fn default_input() -> Option<AudioObjectId> {
    read_u32(SYSTEM_OBJECT, DEFAULT_INPUT).filter(|id| *id != 0)
}

pub fn device_name(device: AudioObjectId) -> Option<String> {
    read_string(device, NAME).filter(|n| !n.trim().is_empty())
}

pub fn device_uid(device: AudioObjectId) -> Option<String> {
    read_string(device, UID)
}

pub fn input_devices() -> Vec<MicDevice> {
    let address = address(DEVICES, SCOPE_GLOBAL);
    let Some(size) = data_size(SYSTEM_OBJECT, &address) else {
        return Vec::new();
    };
    let count = size as usize / std::mem::size_of::<AudioObjectId>();
    let mut ids = vec![0 as AudioObjectId; count];
    let mut size = size;
    // SAFETY: `ids` has room for `size` bytes of device ids.
    let status = unsafe {
        AudioObjectGetPropertyData(
            SYSTEM_OBJECT,
            &address,
            0,
            std::ptr::null(),
            &mut size,
            ids.as_mut_ptr().cast(),
        )
    };
    if status != 0 {
        return Vec::new();
    }
    ids.truncate(size as usize / std::mem::size_of::<AudioObjectId>());
    let default = default_input();
    ids.into_iter()
        .filter(|id| input_channels(*id) > 0)
        .filter_map(|id| {
            Some(MicDevice {
                id: device_uid(id)?,
                label: device_name(id).unwrap_or_default(),
                is_built_in: read_u32(id, TRANSPORT_TYPE) == Some(TRANSPORT_BUILT_IN),
                is_default: default == Some(id),
            })
        })
        .collect()
}
