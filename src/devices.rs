use crate::{model::Reading, vendor};
use hidapi::{DeviceInfo, HidApi, HidDevice};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Devices::HumanInterfaceDevice::{HidD_GetSerialNumberString, HidD_SetOutputReport},
    Devices::{
        DeviceAndDriverInstallation::*,
        Properties::{DEVPKEY_Device_ContainerId, DEVPKEY_Device_Parent},
    },
    Foundation::{
        CloseHandle, DEVPROPKEY, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
    },
    Storage::FileSystem::{
        CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        ReadFile,
    },
    System::{
        IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
        Threading::{CreateEventW, ResetEvent, WaitForSingleObject},
    },
};

pub(crate) fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn raw_serial_number(info: &DeviceInfo) -> String {
    // hidapi fills absent USB/Bluetooth serials from PnP IDs. Query the HID
    // descriptor directly so a topology/path ID is never presented as an SN.
    unsafe {
        let handle = CreateFileW(
            wide(&info.path().to_string_lossy()).as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return String::new();
        }
        let mut buffer = [0u16; 256];
        let valid = HidD_GetSerialNumberString(
            handle,
            buffer.as_mut_ptr().cast(),
            std::mem::size_of_val(&buffer) as u32,
        );
        CloseHandle(handle);
        if !valid {
            return String::new();
        }
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end]).trim().into()
    }
}

/// Run `probe` against a fresh device-info set that is always destroyed.
fn with_device_set<T>(probe: impl FnOnce(HDEVINFO) -> Option<T>) -> Option<T> {
    unsafe {
        let set = SetupDiCreateDeviceInfoList(std::ptr::null(), std::ptr::null_mut());
        if set == -1 {
            return None;
        }
        let result = probe(set);
        SetupDiDestroyDeviceInfoList(set);
        result
    }
}

/// Read one device property from a devnode already opened in `set`, into
/// `buffer`, returning the property size in bytes.
fn node_property(
    set: HDEVINFO,
    device: &SP_DEVINFO_DATA,
    key: &DEVPROPKEY,
    buffer: &mut [u8],
) -> Option<usize> {
    let mut kind = 0;
    let mut size = 0;
    (unsafe {
        SetupDiGetDevicePropertyW(
            set,
            device,
            key,
            &mut kind,
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            &mut size,
            0,
        )
    } != 0)
        .then_some(size as usize)
}

/// Resolve the devnode behind a HID interface and read one device property.
fn device_property(info: &DeviceInfo, key: &DEVPROPKEY, buffer: &mut [u8]) -> Option<usize> {
    with_device_set(|set| unsafe {
        let mut interface = SP_DEVICE_INTERFACE_DATA {
            cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
            ..std::mem::zeroed()
        };
        if SetupDiOpenDeviceInterfaceW(
            set,
            wide(&info.path().to_string_lossy()).as_ptr(),
            0,
            &mut interface,
        ) == 0
        {
            return None;
        }
        let mut needed = 0;
        SetupDiGetDeviceInterfaceDetailW(
            set,
            &interface,
            std::ptr::null_mut(),
            0,
            &mut needed,
            std::ptr::null_mut(),
        );
        if needed == 0 {
            return None;
        }
        // Aligned storage for the variable-size structure.
        let mut detail = vec![0u64; (needed as usize).div_ceil(8)];
        let detail_ptr = detail
            .as_mut_ptr()
            .cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
        (*detail_ptr).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
        let mut device = SP_DEVINFO_DATA {
            cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
            ..std::mem::zeroed()
        };
        if SetupDiGetDeviceInterfaceDetailW(
            set,
            &interface,
            detail_ptr,
            needed,
            std::ptr::null_mut(),
            &mut device,
        ) == 0
        {
            return None;
        }
        node_property(set, &device, key, buffer)
    })
}

/// Windows groups all interfaces of a physical receiver using its ContainerId.
/// This is an internal local identifier, never displayed as a hardware serial.
pub(crate) fn container_id(info: &DeviceInfo) -> String {
    let mut value = [0u8; 16];
    let read = device_property(info, &DEVPKEY_Device_ContainerId, &mut value).unwrap_or(0);
    if read == value.len() && value != [0; 16] {
        return value.iter().map(|b| format!("{b:02x}")).collect();
    }
    info.path().to_string_lossy().into_owned()
}

/// Parse a UTF-16LE device property buffer into a String.
fn property_string(buffer: &[u8]) -> String {
    let units: Vec<u16> = buffer
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    let end = units.iter().position(|&c| c == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

/// Read one device property from a devnode addressed by instance id.
fn instance_property(instance: &[u16], key: &DEVPROPKEY, buffer: &mut [u8]) -> Option<usize> {
    with_device_set(|set| unsafe {
        let mut device = SP_DEVINFO_DATA {
            cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
            ..std::mem::zeroed()
        };
        if SetupDiOpenDeviceInfoW(set, instance.as_ptr(), std::ptr::null_mut(), 0, &mut device) == 0
        {
            return None;
        }
        node_property(set, &device, key, buffer)
    })
}

/// The physical USB node's location suffix (e.g. 6&3af0f9ce&0&8) tracks the
/// port. The immediate parent of a HID interface differs per firmware
/// personality - composite modes add a &MI_ function node and the instance
/// embeds the personality pid - so the chain is climbed to the node without
/// &MI_, whose location suffix survives re-enumeration and the 2104/2106
/// switch. The suffix is case-normalized: Windows varies it between runs.
fn port_location(info: &DeviceInfo) -> String {
    let fallback = || info.path().to_string_lossy().into_owned();
    let mut buffer = [0u8; 1024];
    let mut node = match device_property(info, &DEVPKEY_Device_Parent, &mut buffer) {
        Some(read) if read >= 2 && read <= buffer.len() => property_string(&buffer[..read]),
        _ => return fallback(),
    };
    for _ in 0..3 {
        if !node.contains("&MI_") {
            break;
        }
        let wide: Vec<u16> = node.encode_utf16().chain(Some(0)).collect();
        match instance_property(&wide, &DEVPKEY_Device_Parent, &mut buffer) {
            Some(read) if read >= 2 && read <= buffer.len() => {
                node = property_string(&buffer[..read]);
            }
            _ => break,
        }
    }
    let location = node.rsplit('\\').next().unwrap_or_default().to_lowercase();
    if location.is_empty() {
        fallback()
    } else {
        location
    }
}

#[derive(Debug, Serialize)]
pub struct UsbIdentifiers {
    pub hardware_name: String,
    pub vid: u16,
    pub pid: u16,
}

fn receiver_interfaces(api: &HidApi) -> BTreeMap<String, Vec<&DeviceInfo>> {
    let mut groups: BTreeMap<String, Vec<&DeviceInfo>> = BTreeMap::new();
    for info in api
        .device_list()
        .filter(|d| vendor::ADAPTERS.iter().any(|adapter| adapter.matches(d)))
    {
        groups.entry(container_id(info)).or_default().push(info);
    }
    for interfaces in groups.values_mut() {
        // FF05 first: the ATK dongle's protocol interface.
        interfaces.sort_by_key(|d| d.usage_page() != 0xff05);
    }
    groups
}

/// Enumerate the actual USB identifiers without opening protocol handles or
/// sending battery/configuration reports. One result per physical receiver;
/// Bluetooth audio devices are listed alongside the receivers.
pub fn enumerate_identifiers() -> Result<Vec<UsbIdentifiers>, String> {
    let api = HidApi::new().map_err(|_| "暂时无法读取设备，请稍后重试".to_string())?;
    let mut identifiers: Vec<_> = receiver_interfaces(&api)
        .values()
        .map(|interfaces| {
            let info = interfaces[0];
            UsbIdentifiers {
                hardware_name: info.product_string().unwrap_or("未提供硬件名称").into(),
                vid: info.vendor_id(),
                pid: info.product_id(),
            }
        })
        .collect();
    identifiers.extend(
        crate::bluetooth::audio_devices()
            .into_iter()
            .map(|reading| UsbIdentifiers {
                hardware_name: reading.hardware_name,
                vid: reading.vid,
                pid: reading.pid,
            }),
    );
    Ok(identifiers)
}

pub(crate) fn open_pair(api: &HidApi, info: &DeviceInfo) -> Result<(HidDevice, HidDevice), String> {
    // Keep input on an independent overlapped handle; never mix an outstanding
    // read with writes/IOCTL on the same handle.
    let reader = api.open_path(info.path()).map_err(|e| e.to_string())?;
    let writer = api.open_path(info.path()).map_err(|e| e.to_string())?;
    Ok((reader, writer))
}

pub fn send(writer: &HidDevice, data: &[u8]) -> Result<(), String> {
    writer
        .send_output_report(data)
        .or_else(|_| {
            writer.write(data).and_then(|n| {
                // windows-native returns 0 when WriteFile completes synchronously.
                if n == data.len() || n == 0 {
                    Ok(())
                } else {
                    Err(hidapi::HidError::HidApiError {
                        message: "设备写入长度不足".into(),
                    })
                }
            })
        })
        .map_err(|e| e.to_string())
}

pub fn drain(reader: &HidDevice) {
    let mut bytes = [0u8; 256];
    // Bounded even when a device continuously sends input.
    for _ in 0..32 {
        if !matches!(reader.read_timeout(&mut bytes, 0), Ok(n) if n > 0) {
            break;
        }
    }
}

/// KZZI keyboards behind a Compx 2.4G receiver. This interface only answers
/// requests written through the control endpoint (HidD_SetOutputReport) and
/// hidapi's buffer handling fails there (error 1784), so the exchange runs on
/// raw Windows handles: one handle for writes, one overlapped handle for reads.
pub struct KzziLink {
    writer: *mut std::ffi::c_void,
    reader: *mut std::ffi::c_void,
    event: *mut std::ffi::c_void,
}

impl KzziLink {
    pub fn open(path: &str) -> Result<Self, String> {
        let path = wide(path);
        let writer = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if writer == INVALID_HANDLE_VALUE {
            return Err("无法打开键盘接口".into());
        }
        let reader = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                std::ptr::null_mut(),
            )
        };
        if reader == INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(writer) };
            return Err("无法读取键盘接口".into());
        }
        let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if event.is_null() {
            unsafe {
                CloseHandle(writer);
                CloseHandle(reader);
            }
            return Err("无法创建键盘读事件".into());
        }
        Ok(Self {
            writer,
            reader,
            event,
        })
    }

    pub fn send(&self, frame: &[u8; 20]) -> Result<(), String> {
        let ok =
            unsafe { HidD_SetOutputReport(self.writer, frame.as_ptr().cast(), frame.len() as u32) };
        if ok {
            Ok(())
        } else {
            Err("键盘命令发送失败".into())
        }
    }

    /// Cancel a pending overlapped read so the handle stays reusable.
    fn cancel(&self, pending: &mut OVERLAPPED) {
        unsafe {
            CancelIoEx(self.reader, pending as *mut _ as *mut _);
            WaitForSingleObject(self.event, 200);
            let mut read = 0u32;
            GetOverlappedResult(self.reader, pending, &mut read, 0);
        }
    }

    pub fn read(&self, buffer: &mut [u8], timeout: Duration) -> Option<usize> {
        let mut pending: OVERLAPPED = unsafe { std::mem::zeroed() };
        pending.hEvent = self.event;
        let mut read = 0u32;
        unsafe { ResetEvent(self.event) };
        let started = unsafe {
            ReadFile(
                self.reader,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut read,
                &mut pending,
            )
        };
        if started != 0 {
            return (read > 0).then_some(read as usize);
        }
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.cancel(&mut pending);
                return None;
            }
            let wait =
                unsafe { WaitForSingleObject(self.event, remaining.as_millis().max(1) as u32) };
            if wait == WAIT_OBJECT_0 {
                let done = unsafe { GetOverlappedResult(self.reader, &pending, &mut read, 0) };
                return (done != 0 && read > 0).then_some(read as usize);
            }
        }
    }

    pub fn drain(&self) {
        let mut buffer = [0u8; 64];
        for _ in 0..32 {
            if self.read(&mut buffer, Duration::from_millis(8)).is_none() {
                break;
            }
        }
    }
}

impl Drop for KzziLink {
    fn drop(&mut self) {
        unsafe {
            CancelIoEx(self.reader, std::ptr::null_mut());
            CloseHandle(self.event);
            CloseHandle(self.reader);
            CloseHandle(self.writer);
        }
    }
}

pub fn scan() -> Result<Vec<Reading>, String> {
    let api = HidApi::new().map_err(|_| "暂时无法读取设备，请稍后刷新".to_string())?;
    let mut result = Vec::new();
    for (_, interfaces) in receiver_interfaces(&api) {
        let info = interfaces[0];
        let Some(adapter) = vendor::ADAPTERS.iter().find(|a| a.matches(info)) else {
            continue;
        };
        // One physical receiver can report several paired devices; only then
        // do the readings carry slot marks, so single-device receivers keep
        // their plain port id across scans.
        let outcomes = adapter.read_all(&api, info);
        let multi = outcomes.len() > 1;
        for outcome in outcomes {
            let mut physical_id = port_location(info);
            if multi && let Some(slot) = outcome.slot {
                physical_id.push('#');
                physical_id.push_str(&slot.to_string());
            }
            result.push(Reading {
                physical_id,
                kind: adapter.kind().into(),
                hardware_name: outcome
                    .hardware_name
                    .unwrap_or_else(|| info.product_string().unwrap_or("未提供硬件名称").into()),
                serial: raw_serial_number(info),
                uid: outcome.uid,
                vid: info.vendor_id(),
                pid: info.product_id(),
                battery: outcome.battery,
                no_battery: outcome.no_battery,
                message: outcome.message,
            });
        }
    }
    result.extend(crate::bluetooth::audio_devices());
    result.sort_by_key(|r| {
        (
            r.kind != "mouse",
            r.hardware_name.clone(),
            r.physical_id.clone(),
        )
    });
    Ok(result)
}
