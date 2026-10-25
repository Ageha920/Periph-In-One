//! ATK / VXE mice behind the 373B:1216 nano dongle: FF05 protocol interface
//! preferred, FF02 fallback interface of the same receiver.

use crate::{
    devices::{drain, open_pair, send},
    protocol,
    vendor::{Capabilities, Outcome, VendorAdapter},
};
use hidapi::{DeviceInfo, HidApi};
use std::time::{Duration, Instant};

pub struct AtkMouse;

impl VendorAdapter for AtkMouse {
    fn matches(&self, info: &DeviceInfo) -> bool {
        info.vendor_id() == 0x373b
            && info.product_id() == 0x1216
            && matches!(info.usage_page(), 0xff05 | 0xff02)
    }

    fn kind(&self) -> &'static str {
        "mouse"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::BATTERY
    }

    fn read(&self, api: &HidApi, info: &DeviceInfo) -> Outcome {
        match read_battery(api, info) {
            Ok(level) => Outcome {
                battery: Some(level),
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

fn read_battery(api: &HidApi, info: &DeviceInfo) -> Result<u8, String> {
    let (reader, writer) = open_pair(api, info)?;
    let mut buffer = [0; 256];
    for _ in 0..3 {
        drain(&reader);
        if info.usage_page() == 0xff05 {
            send(&writer, &protocol::atk_request())?;
        } else {
            send(&writer, &protocol::atk_ff02_request())?;
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
                protocol::atk_ff02_battery(&buffer[..n])
            };
            if let Some(level) = battery {
                return Ok(level);
            }
        }
    }
    Err("鼠标未响应，请移动鼠标后刷新".into())
}
