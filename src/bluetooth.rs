//! Bluetooth classic audio devices (BTHENUM). Windows records the battery a
//! headset reports over HFP/AVRCP on the Hands-Free AG service node — a
//! sibling of the device node, keyed by the shared MAC address — so the
//! enumeration collects device nodes and battery-carrying children in one
//! pass and joins them. There is no HID and no reliable serial, so the MAC
//! address is the stable physical identity. Only audio/video major-class
//! devices are listed.

use crate::{devices::wide, model::Reading};
use windows_sys::Win32::Devices::{DeviceAndDriverInstallation::*, Properties::*};
use windows_sys::Win32::Foundation::DEVPROPKEY;
use windows_sys::core::GUID;

/// DEVPKEY_Bluetooth_* (bthdef.h): {2BD67D8B-8BEB-48D5-87E0-6CDA3428040A}.
const BLUETOOTH_PROPKEY_GUID: GUID = GUID::from_u128(0x2bd67d8b_8beb_48d5_87e0_6cda3428040a);
fn bluetooth_propkey(pid: u32) -> DEVPROPKEY {
    DEVPROPKEY {
        fmtid: BLUETOOTH_PROPKEY_GUID,
        pid,
    }
}

/// Battery a headset reported over HFP/AVRCP, as recorded by Windows on the
/// Hands-Free AG service node: {104EA319-6EE2-4701-BD47-8DDBF425BBE5},
/// property 2, a single byte holding the percentage.
const BLUETOOTH_BATTERY_KEY: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x104ea319_6ee2_4701_bd47_8ddbf425bbe5),
    pid: 2,
};

/// Live connection state on the device node: {83DA6326-97A6-4088-9453-
/// A1923F573B29}, property 15. BTHENUM nodes survive a disconnect — including
/// the Hands-Free node with its stale battery byte — so this flag is the only
/// reliable "actually connected" signal.
const BLUETOOTH_ISCONNECTED_KEY: DEVPROPKEY = DEVPROPKEY {
    fmtid: GUID::from_u128(0x83da6326_97a6_4088_9453_a1923f573b29),
    pid: 15,
};

unsafe fn device_property(set: isize, data: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<Vec<u8>> {
    let mut kind = 0;
    let mut needed = 0;
    unsafe {
        if SetupDiGetDevicePropertyW(
            set,
            data,
            key,
            &mut kind,
            std::ptr::null_mut(),
            0,
            &mut needed,
            0,
        ) == 0
            && needed == 0
        {
            return None;
        }
        let mut buffer = vec![0u8; needed as usize];
        if SetupDiGetDevicePropertyW(
            set,
            data,
            key,
            &mut kind,
            buffer.as_mut_ptr(),
            needed,
            std::ptr::null_mut(),
            0,
        ) == 0
        {
            return None;
        }
        Some(buffer)
    }
}

fn property_string(set: isize, data: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> String {
    let bytes = unsafe { device_property(set, data, key) }.unwrap_or_default();
    let utf16: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    let end = utf16.iter().position(|&c| c == 0).unwrap_or(utf16.len());
    String::from_utf16_lossy(&utf16[..end]).trim().to_string()
}

fn property_u16(set: isize, data: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> u16 {
    let bytes = unsafe { device_property(set, data, key) }.unwrap_or_default();
    bytes
        .first_chunk::<2>()
        .map_or(0, |pair| u16::from_le_bytes(*pair))
}

fn property_u32(set: isize, data: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> u32 {
    let bytes = unsafe { device_property(set, data, key) }.unwrap_or_default();
    bytes
        .first_chunk::<4>()
        .map_or(0, |quad| u32::from_le_bytes(*quad))
}

pub fn audio_devices() -> Vec<Reading> {
    /// A remote device node ("BTHENUM\DEV_…") or a battery-carrying service
    /// child of one, both identified by the shared Bluetooth MAC address.
    struct BtNode {
        address: String,
        is_device: bool,
        name: String,
        vid: u16,
        pid: u16,
        audio: bool,
        connected: bool,
        battery: Option<u8>,
    }
    let mut nodes: Vec<BtNode> = Vec::new();
    unsafe {
        // Enumerating the BTHENUM bus instead of the Bluetooth device class:
        // the battery-carrying Hands-Free node belongs to a different device
        // class than the device node it belongs to. A NULL class requires
        // DIGCF_ALLCLASSES.
        let enumerator = wide("BTHENUM");
        let set = SetupDiGetClassDevsW(
            std::ptr::null(),
            enumerator.as_ptr(),
            std::ptr::null_mut(),
            DIGCF_PRESENT | DIGCF_ALLCLASSES,
        );
        if set == -1 {
            return Vec::new();
        }
        let mut index = 0u32;
        loop {
            let mut data = SP_DEVINFO_DATA {
                cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
                ..std::mem::zeroed()
            };
            if SetupDiEnumDeviceInfo(set, index, &mut data) == 0 {
                break;
            }
            index += 1;
            let instance = property_string(set, &data, &DEVPKEY_Device_InstanceId);
            // The MAC is the twelve-hex-digit token in the instance id; the
            // device node keeps it right after "DEV_", service children carry
            // it in their parent path segment.
            let Some(address) = instance
                .split(['\\', '&', '_'])
                .find(|token| token.len() == 12 && token.bytes().all(|b| b.is_ascii_hexdigit()))
                .map(str::to_ascii_uppercase)
            else {
                continue;
            };
            let is_device = instance.starts_with("BTHENUM\\DEV_");
            let battery = device_property(set, &data, &BLUETOOTH_BATTERY_KEY)
                .and_then(|bytes| bytes.first().copied())
                .filter(|level| (1..=100).contains(level));
            match nodes
                .iter_mut()
                .find(|node| node.address == address && node.is_device == is_device)
            {
                Some(node) if !node.is_device => {
                    node.battery = node.battery.or(battery);
                }
                _ => {
                    // Bluetooth class of device, major class in bits 8..13:
                    // 4 = audio/video. Service children inherit the parent's
                    // class through the same property.
                    let class = property_u32(set, &data, &bluetooth_propkey(4));
                    let name = property_string(set, &data, &DEVPKEY_Device_FriendlyName);
                    // Only the device node carries the live connection flag;
                    // treat a missing flag as connected so a future Windows
                    // variant cannot silently hide devices.
                    let connected = is_device
                        && device_property(set, &data, &BLUETOOTH_ISCONNECTED_KEY)
                            .and_then(|bytes| bytes.first().copied())
                            .map(|flag| flag != 0)
                            .unwrap_or(true);
                    nodes.push(BtNode {
                        address,
                        is_device,
                        name,
                        vid: property_u16(set, &data, &bluetooth_propkey(7)),
                        pid: property_u16(set, &data, &bluetooth_propkey(8)),
                        audio: (class >> 8) & 0x1f == 0x04,
                        connected,
                        battery,
                    });
                }
            }
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    nodes
        .iter()
        .filter(|node| node.is_device && node.audio && node.connected)
        .map(|device| {
            let battery = nodes
                .iter()
                .filter(|node| !node.is_device && node.address == device.address)
                .find_map(|node| node.battery)
                .or(device.battery);
            Reading {
                physical_id: format!("bt:{}", device.address),
                kind: "bt-headset".into(),
                hardware_name: if device.name.is_empty() {
                    "蓝牙耳机".into()
                } else {
                    device.name.clone()
                },
                serial: String::new(),
                uid: String::new(),
                vid: device.vid,
                pid: device.pid,
                battery,
                // The Windows connection flag verified liveness; a headset
                // that has not reported a level stays connected with no value.
                no_battery: true,
                message: "已连接".into(),
            }
        })
        .collect()
}
