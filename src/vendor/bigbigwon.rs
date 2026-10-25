//! BIGBIG WON controllers (GALE family and other Zikway models) over the
//! FF7A vendor interface. GetDevicePowerInfo answers only while the controller
//! is linked and running its composite gamepad personality; while linked,
//! GetUUID reports the 8-byte per-unit identity that anchors the record
//! across personalities and ports.

use crate::{
    devices::{drain, open_pair, send},
    protocol,
    vendor::{Capabilities, Outcome, VendorAdapter},
};
use hidapi::{DeviceInfo, HidApi, HidDevice};
use std::time::{Duration, Instant};

pub struct BigbigwonGamepad;

impl VendorAdapter for BigbigwonGamepad {
    fn matches(&self, info: &DeviceInfo) -> bool {
        // The vendor interface keeps usage page FF7A across firmware
        // personalities (2104 linked, 2106 idle), so the PID is not pinned.
        info.vendor_id() == 0x413d && info.usage_page() == 0xff7a && info.usage() == 1
    }

    fn kind(&self) -> &'static str {
        "gamepad"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::BATTERY
    }

    fn read(&self, api: &HidApi, info: &DeviceInfo) -> Outcome {
        match probe(api, info) {
            Ok((level, uid)) => Outcome {
                battery: level,
                uid,
                message: "已连接".into(),
                ..Outcome::default()
            },
            Err(error) => Outcome {
                message: if error.contains("未响应") {
                    error
                } else {
                    "读取失败，请重新连接设备后刷新".into()
                },
                ..Outcome::default()
            },
        }
    }
}

fn probe(api: &HidApi, info: &DeviceInfo) -> Result<(Option<u8>, String), String> {
    let (reader, writer) = open_pair(api, info)?;
    let mut buffer = [0; 256];
    for _ in 0..3 {
        drain(&reader);
        send(&writer, &protocol::bigbigwon_request())?;
        let end = Instant::now() + Duration::from_millis(650);
        while Instant::now() < end {
            let n = reader
                .read_timeout(&mut buffer, 120)
                .map_err(|e| e.to_string())?;
            if n == 0 {
                continue;
            }
            if let Some(level) = protocol::bigbigwon_battery(&buffer[..n]) {
                return Ok((Some(level), gamepad_uid(&reader, &writer)));
            }
        }
    }
    Err("手柄未响应，请唤醒手柄后刷新".into())
}

fn gamepad_uid(reader: &HidDevice, writer: &HidDevice) -> String {
    let mut buffer = [0u8; 256];
    for _ in 0..2 {
        drain(reader);
        if send(writer, &protocol::bigbigwon_uuid_request()).is_err() {
            return String::new();
        }
        let end = Instant::now() + Duration::from_millis(400);
        while Instant::now() < end {
            let Ok(n) = reader.read_timeout(&mut buffer, 80) else {
                return String::new();
            };
            if n == 0 {
                continue;
            }
            if let Some(uuid) = protocol::bigbigwon_uuid(&buffer[..n]) {
                return uuid.iter().map(|b| format!("{b:02x}")).collect();
            }
        }
    }
    String::new()
}
