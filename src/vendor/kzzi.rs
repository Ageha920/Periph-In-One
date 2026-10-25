//! KZZI keyboards behind a Compx 2.4G receiver (3554:FA09). The receiver
//! answers identity itself; battery queries are relayed to the keyboard and
//! only models whose firmware implements them reply. The interface only
//! answers requests written through the control endpoint (hidapi fails there
//! with error 1784), so the exchange runs on `devices::KzziLink` raw handles.

use crate::{
    devices::KzziLink,
    protocol,
    vendor::{Capabilities, Outcome, VendorAdapter},
};
use hidapi::{DeviceInfo, HidApi};
use std::time::{Duration, Instant};

pub struct KzziKeyboard;

impl VendorAdapter for KzziKeyboard {
    fn matches(&self, info: &DeviceInfo) -> bool {
        info.vendor_id() == 0x3554
            && info.product_id() == 0xfa09
            && info.usage_page() == 0xff02
            && info.usage() == 2
    }

    fn kind(&self) -> &'static str {
        "keyboard"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::BATTERY
    }

    fn read(&self, _api: &HidApi, info: &DeviceInfo) -> Outcome {
        match probe(info) {
            Ok((battery, name)) => {
                let identified = !name.is_empty();
                Outcome {
                    battery,
                    hardware_name: identified.then_some(name),
                    message: if battery.is_some() || identified {
                        "已连接".into()
                    } else {
                        "键盘未响应，请确认键盘已开机".into()
                    },
                    ..Outcome::default()
                }
            }
            Err(_) => Outcome {
                message: "读取失败，请重新连接设备后刷新".into(),
                ..Outcome::default()
            },
        }
    }
}

fn probe(info: &DeviceInfo) -> Result<(Option<u8>, String), String> {
    let link = KzziLink::open(&info.path().to_string_lossy())?;
    let mut buffer = [0u8; 64];
    let mut name = String::new();
    for _ in 0..3 {
        link.drain();
        link.send(&protocol::kzzi_identity_request())?;
        let end = Instant::now() + Duration::from_millis(600);
        while name.is_empty() && Instant::now() < end {
            if let Some(n) = link.read(&mut buffer, Duration::from_millis(120))
                && let Some(psd) = protocol::kzzi_identity(&buffer[..n])
            {
                name = protocol::kzzi_model_name(&psd)
                    .unwrap_or("无线键盘")
                    .to_string();
            }
        }
        if !name.is_empty() {
            break;
        }
    }
    for _ in 0..3 {
        link.drain();
        link.send(&protocol::kzzi_request(0x4a))?;
        let end = Instant::now() + Duration::from_millis(900);
        while Instant::now() < end {
            if let Some(n) = link.read(&mut buffer, Duration::from_millis(120))
                && let Some((level, _)) = protocol::kzzi_battery(&buffer[..n])
            {
                return Ok((Some(level), name));
            }
        }
    }
    Ok((None, name))
}
