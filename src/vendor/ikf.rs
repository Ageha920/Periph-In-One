//! iKF/Cm USB headsets over the FF00/1 vendor interface (38B1 VID). The
//! official client (iKF Audio) polls battery with the FF-framed command
//! {FA, 0C} packed into a 64-byte report id 2; the answer returns on the
//! input pipe as report id 3 with the level in property 0x0C. Verified live
//! on the 38B1:3419 V11 Pro; the VID also carries the 3412/341E models of
//! the vendor's APO driver, so the PID is not pinned.

use crate::{
    devices::{drain, open_pair, send},
    protocol,
    vendor::{Capabilities, Outcome, VendorAdapter},
};
use hidapi::{DeviceInfo, HidApi};
use std::time::{Duration, Instant};

pub struct IkfHeadset;

impl VendorAdapter for IkfHeadset {
    fn matches(&self, info: &DeviceInfo) -> bool {
        info.vendor_id() == 0x38b1 && info.usage_page() == 0xff00 && info.usage() == 1
    }

    fn kind(&self) -> &'static str {
        "headset"
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
            Err(_) => Outcome {
                message: "耳机未响应，请确认耳机已开机".into(),
                ..Outcome::default()
            },
        }
    }
}

fn read_battery(api: &HidApi, info: &DeviceInfo) -> Result<u8, String> {
    let (reader, writer) = open_pair(api, info)?;
    let mut buffer = [0u8; 64];
    for _ in 0..3 {
        drain(&reader);
        send(&writer, &protocol::ikf_battery_query(0))?;
        let end = Instant::now() + Duration::from_millis(900);
        while Instant::now() < end {
            if let Ok(n) = reader.read_timeout(&mut buffer, 120)
                && n > 0
                && let Some(level) = protocol::ikf_battery(&buffer[..n])
            {
                return Ok(level);
            }
        }
    }
    Err("耳机未响应".into())
}
