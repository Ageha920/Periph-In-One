//! Probe an Aula command-protocol keyboard (VID 0x0C45 SDK channel) for the
//! battery exchange implemented in the scan adapter. Read-only: the two
//! battery commands from Aula Hub 2.2.3 (0x11 primary, 0x51 fallback, both
//! mode 0x16) and nothing else. The adapter is written from the deobfuscated
//! renderer SDK and has not seen hardware — this probe is the missing step.
//! Run with the dongle plugged in: `cargo run --example probe_aula`.
use hidapi::HidApi;
use periph_in_one::protocol;
use std::time::{Duration, Instant};

fn dump(label: &str, frame: &[u8]) {
    let hex = frame
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ");
    println!("{label} {hex}");
}

fn main() {
    let api = HidApi::new().expect("hidapi");
    let Some(info) = api
        .device_list()
        .find(|info| {
            info.vendor_id() == 0x0c45
                && matches!(info.product_id(), 0xfef9 | 0xfefc | 0xfefe)
                && matches!(
                    info.usage_page(),
                    0xff60 | 0xff67 | 0xff68 | 0xff70 | 0xff80
                )
                && matches!(info.usage(), 0x01 | 0x61 | 0x71)
        })
        .cloned()
    else {
        println!("未发现狼蛛命令协议键盘（0x0C45:FEF9/FEFC/FEFE）");
        return;
    };
    println!(
        "键盘 {:04X}:{:04X} usagePage {:04X}/usage {:02X} —— {}",
        info.vendor_id(),
        info.product_id(),
        info.usage_page(),
        info.usage(),
        info.product_string().unwrap_or("未提供硬件名称")
    );
    let device = api.open_path(info.path()).expect("打开 SDK 通道");
    for cmd in [protocol::AULA_CMD_BATTERY, protocol::AULA_CMD_BATTERY_ALT] {
        let request = protocol::aula_command_request(cmd, protocol::AULA_MODE_BATTERY);
        dump("请求 ", &request);
        let mut wired = Vec::with_capacity(33);
        wired.push(0);
        wired.extend_from_slice(&request);
        let mut buffer = [0u8; 64];
        while device.read_timeout(&mut buffer, 30).unwrap_or(0) > 0 {}
        device.write(&wired).expect("写入");
        let end = Instant::now() + Duration::from_secs(2);
        let mut answered = false;
        while Instant::now() < end {
            match device.read_timeout(&mut buffer, 100) {
                Ok(n) if n > 0 => {
                    dump("应答 ", &buffer[..n]);
                    if let Some((level, charging)) =
                        protocol::aula_command_battery(&buffer[..n], cmd)
                    {
                        println!("  → 电量 {level}%，充电状态 {charging}");
                        answered = true;
                        break;
                    }
                }
                _ => continue,
            }
        }
        if !answered {
            println!("  → 2s 内无有效应答");
        }
    }
}
