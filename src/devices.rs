use crate::{model::Reading, protocol};
use hidapi::{DeviceInfo, HidApi, HidDevice};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Devices::{DeviceAndDriverInstallation::*, Properties::DEVPKEY_Device_ContainerId},
    Foundation::DEVPROPKEY,
};

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

/// Windows groups all interfaces of a physical receiver using its ContainerId.
/// This is an internal local identifier, never displayed as a hardware serial.
fn container_id(info: &DeviceInfo) -> String {
    unsafe {
        let set = SetupDiCreateDeviceInfoList(std::ptr::null(), std::ptr::null_mut());
        if set == -1 {
            return info.path().to_string_lossy().into_owned();
        }
        let result = (|| {
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
            let mut value = [0u8; 16];
            let mut kind = 0;
            let key: DEVPROPKEY = DEVPKEY_Device_ContainerId;
            if SetupDiGetDevicePropertyW(
                set,
                &device,
                &key,
                &mut kind,
                value.as_mut_ptr(),
                16,
                std::ptr::null_mut(),
                0,
            ) == 0
                || value == [0; 16]
            {
                return None;
            }
            Some(value.iter().map(|b| format!("{b:02x}")).collect::<String>())
        })();
        SetupDiDestroyDeviceInfoList(set);
        result.unwrap_or_else(|| info.path().to_string_lossy().into_owned())
    }
}

fn supported(info: &DeviceInfo) -> bool {
    (info.vendor_id() == 0x373b
        && info.product_id() == 0x1216
        && matches!(info.usage_page(), 0xff05 | 0xff02))
        || (info.vendor_id() == 0x3654
            && info.product_id() == 0x6901
            && info.usage_page() == 0xff02
            && info.usage() == 2)
}

fn open_pair(api: &HidApi, info: &DeviceInfo) -> Result<(HidDevice, HidDevice), String> {
    // Keep input on an independent overlapped handle; never mix an outstanding
    // read with writes/IOCTL on the same handle.
    let reader = api.open_path(info.path()).map_err(|e| e.to_string())?;
    let writer = api.open_path(info.path()).map_err(|e| e.to_string())?;
    Ok((reader, writer))
}

fn send(writer: &HidDevice, data: &[u8]) -> Result<(), String> {
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

fn drain(reader: &HidDevice) {
    let mut bytes = [0u8; 256];
    // Bounded even when a device continuously sends input.
    for _ in 0..32 {
        if !matches!(reader.read_timeout(&mut bytes, 0), Ok(n) if n > 0) {
            break;
        }
    }
}

fn mouse(api: &HidApi, info: &DeviceInfo) -> Result<u8, String> {
    let (reader, writer) = open_pair(api, info)?;
    let mut buffer = [0; 256];
    for _ in 0..3 {
        drain(&reader);
        if info.usage_page() == 0xff05 {
            send(&writer, &protocol::atk_request())?;
        } else {
            let mut request = [0; 17];
            request[0] = 8;
            request[1] = 4;
            // Preserves the verified Python report construction (17 bytes).
            send(&writer, &request)?;
        }
        let end = Instant::now() + Duration::from_millis(650);
        while Instant::now() < end {
            let n = reader
                .read_timeout(&mut buffer, 120)
                .map_err(|e| e.to_string())?;
            if n == 0 {
                continue;
            }
            let battery = if info.usage_page() == 0xff05 {
                protocol::atk_battery(&buffer[..n])
            } else {
                (n > 6 && buffer[6] <= 100).then(|| buffer[6])
            };
            if let Some(level) = battery {
                return Ok(level);
            }
        }
    }
    Err("鼠标未响应，请移动鼠标后刷新".into())
}

fn headset(api: &HidApi, info: &DeviceInfo) -> Result<(Option<u8>, String), String> {
    let (reader, writer) = open_pair(api, info)?;
    let mut battery = None;
    let mut name = String::new();
    let mut buffer = [0; 256];
    for (seq, kind) in [(0, 1), (1, 2)] {
        drain(&reader);
        send(&writer, &protocol::darkrim_query(seq, kind))?;
        let end = Instant::now() + Duration::from_millis(450);
        while Instant::now() < end {
            let n = reader
                .read_timeout(&mut buffer, 100)
                .map_err(|e| e.to_string())?;
            let report = protocol::restore_darkrim_report_id(&buffer[..n]);
            if let Some(tlvs) = protocol::darkrim_tlvs(&report, seq) {
                for (key, value) in tlvs {
                    if key == 1 && !value.is_empty() && value[0] <= 100 {
                        battery = Some(value[0]);
                    }
                    if key == 2 {
                        name = String::from_utf8_lossy(value)
                            .trim_matches(char::from(0))
                            .trim()
                            .to_string();
                    }
                }
            }
        }
    }
    Ok((battery, name))
}

pub fn scan() -> Result<Vec<Reading>, String> {
    let api = HidApi::new().map_err(|e| format!("设备枚举失败：{e}"))?;
    let mut groups: BTreeMap<String, Vec<&DeviceInfo>> = BTreeMap::new();
    for info in api.device_list().filter(|d| supported(d)) {
        groups.entry(container_id(info)).or_default().push(info);
    }
    let mut result = Vec::new();
    for (physical_id, mut interfaces) in groups {
        interfaces.sort_by_key(|d| if d.usage_page() == 0xff05 { 0 } else { 1 });
        let info = interfaces[0];
        let mut reading = Reading {
            physical_id,
            kind: if info.vendor_id() == 0x373b {
                "mouse"
            } else {
                "headset"
            }
            .into(),
            hardware_name: info.product_string().unwrap_or("未提供硬件名称").into(),
            serial: info.serial_number().unwrap_or_default().trim().into(),
            vid: info.vendor_id(),
            pid: info.product_id(),
            battery: None,
            message: String::new(),
        };
        if info.vendor_id() == 0x373b {
            match mouse(&api, info) {
                Ok(level) => {
                    reading.battery = Some(level);
                    reading.message = "已连接".into();
                }
                Err(error) => reading.message = error,
            }
        } else {
            match headset(&api, info) {
                Ok((battery, name)) => {
                    reading.battery = battery;
                    if !name.is_empty() {
                        reading.hardware_name = name;
                    }
                    reading.message = if battery.is_some() {
                        "已连接"
                    } else {
                        "耳机未响应，请检查电源和连接"
                    }
                    .into();
                }
                Err(error) => reading.message = error,
            }
        }
        result.push(reading);
    }
    result.sort_by_key(|r| {
        (
            r.kind != "mouse",
            r.hardware_name.clone(),
            r.physical_id.clone(),
        )
    });
    Ok(result)
}
