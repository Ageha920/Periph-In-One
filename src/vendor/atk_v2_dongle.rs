//! ATK V2 2.4G dongles over the FF02/2 interface: the CompX-VID 0x3554
//! receiver line of ATK V HUB's device table (`receiver` flag). Requests are
//! the FF04 frame layout sent as 17-byte output reports with report id 8;
//! replies arrive as input reports. The FF04 feature-report interface these
//! dongles also expose rejects writes on live hardware (3554:F58E), so this
//! adapter owns the working transport. Verified live on the 1K dongle:
//! GetWirelessMouseOnline answers status 0 with the rf id, GetBatteryLevel
//! status 0, dataValidLen 2, level 60, charging 0, voltage 0x0F51 BE mV.

use crate::{
    devices::{drain, open_pair, send},
    protocol,
    vendor::{Capabilities, Outcome, VendorAdapter},
};
use hidapi::{DeviceInfo, HidApi};
use std::time::{Duration, Instant};

/// Receiver PIDs of the CompX-VID dongle line, from ATK V HUB 2.3.47's
/// WebHID device table (usage page FF04, receiver flag). Only 0xf58e is
/// live-verified; the rest are 已实现未实测. The wired personalities are
/// absent on purpose - their transport is unproven and the FF04 adapter
/// already covers them.
const RECEIVER_PIDS: [u16; 9] = [
    0xf201, 0xf503, 0xf505, 0xf50b, 0xf528, 0xf58a, 0xf58e, 0xf590, 0xfb12,
];

pub struct AtkV2Dongle;

impl VendorAdapter for AtkV2Dongle {
    fn matches(&self, info: &DeviceInfo) -> bool {
        info.vendor_id() == 0x3554
            && info.usage_page() == 0xff02
            && info.usage() == 2
            && RECEIVER_PIDS.contains(&info.product_id())
    }

    fn kind(&self) -> &'static str {
        "mouse"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::BATTERY
    }

    fn read(&self, api: &HidApi, info: &DeviceInfo) -> Outcome {
        match probe(api, info) {
            Ok(outcome) => outcome,
            Err(_) => Outcome {
                message: "读取失败，请重新连接设备后刷新".into(),
                ..Outcome::default()
            },
        }
    }
}

fn outcome(battery: Option<u8>, online: Option<bool>, rf: [u8; 3]) -> Outcome {
    let uid = if rf.iter().any(|&b| b != 0) {
        rf.iter().map(|b| format!("{b:02x}")).collect::<String>()
    } else {
        String::new()
    };
    match (online, battery) {
        (Some(true), Some(level)) => Outcome {
            battery: Some(level),
            uid,
            message: "已连接".into(),
            ..Outcome::default()
        },
        (Some(true), None) => Outcome {
            uid,
            message: "鼠标未响应，请移动鼠标后刷新".into(),
            ..Outcome::default()
        },
        (Some(false), _) => Outcome {
            // The dongle is live but idle: no linked mouse, no battery to show.
            no_battery: true,
            uid,
            message: "接收器在线，未检测到鼠标".into(),
            ..Outcome::default()
        },
        (None, _) => Outcome {
            message: "接收器未响应，请重新插拔后刷新".into(),
            ..Outcome::default()
        },
    }
}

fn probe(api: &HidApi, info: &DeviceInfo) -> Result<Outcome, String> {
    let (reader, writer) = open_pair(api, info)?;
    let mut buffer = [0u8; 64];
    // GetWirelessMouseOnline (3): linked mouse vs idle dongle, plus the rf id
    // used as the per-unit identity.
    let mut online = None;
    let mut rf = [0u8; 3];
    for _ in 0..2 {
        drain(&reader);
        send(&writer, &protocol::atk_ff04_request(0x03))?;
        let end = Instant::now() + Duration::from_millis(600);
        while online.is_none() && Instant::now() < end {
            if let Ok(n) = reader.read_timeout(&mut buffer, 120)
                && n > 0
                && let Some((status, id)) = protocol::atk_v2_ff02_online(&buffer[..n])
            {
                online = Some(status == 1);
                rf = id;
            }
        }
        if online.is_some() {
            break;
        }
    }
    // GetBatteryLevel (4) only reaches a linked mouse; an idle dongle has
    // nothing to ask.
    let mut battery = None;
    if online == Some(true) {
        for _ in 0..3 {
            drain(&reader);
            send(&writer, &protocol::atk_ff04_request(0x04))?;
            let end = Instant::now() + Duration::from_millis(900);
            while battery.is_none() && Instant::now() < end {
                if let Ok(n) = reader.read_timeout(&mut buffer, 120)
                    && n > 0
                    && let Some((level, _)) = protocol::atk_v2_ff02_battery(&buffer[..n])
                {
                    battery = Some(level);
                }
            }
            if battery.is_some() {
                break;
            }
        }
    }
    Ok(outcome(battery, online, rf))
}

#[cfg(test)]
mod tests {
    use super::{RECEIVER_PIDS, outcome};

    #[test]
    fn receiver_pid_table_is_sorted_unique_and_receiver_only() {
        assert_eq!(RECEIVER_PIDS.len(), 9);
        assert!(RECEIVER_PIDS.is_sorted());
        assert!(RECEIVER_PIDS.windows(2).all(|pair| pair[0] != pair[1]));
        // The live-verified 1K dongle stays in the table.
        assert!(RECEIVER_PIDS.contains(&0xf58e));
        // The KZZI receiver shares VID and usage page; only its own adapter
        // may match it.
        assert!(!RECEIVER_PIDS.contains(&0xfa09));
        // Wired personalities stay with the FF04 adapter.
        for wired in [0xf200, 0xf502, 0xf506, 0xf50a, 0xf540, 0xf58c, 0xf58f] {
            assert!(!RECEIVER_PIDS.contains(&wired));
        }
    }

    #[test]
    fn outcome_distinguishes_linked_idle_and_silent_dongles() {
        let linked = outcome(Some(60), Some(true), [0xa5, 0x20, 0xcf]);
        assert_eq!(linked.battery, Some(60));
        assert_eq!(linked.uid, "a520cf");
        assert_eq!(linked.message, "已连接");

        let silent_mouse = outcome(None, Some(true), [0xa5, 0x20, 0xcf]);
        assert_eq!(silent_mouse.battery, None);
        assert!(!silent_mouse.no_battery);
        assert_eq!(silent_mouse.message, "鼠标未响应，请移动鼠标后刷新");

        let idle = outcome(None, Some(false), [0xa5, 0x20, 0xcf]);
        assert!(idle.no_battery);
        assert_eq!(idle.message, "接收器在线，未检测到鼠标");

        let unresponsive = outcome(None, None, [0, 0, 0]);
        assert_eq!(unresponsive.message, "接收器未响应，请重新插拔后刷新");
        assert_eq!(unresponsive.uid, "");
    }
}
