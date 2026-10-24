//! Probe the Zikway/BIGBIG WON FF7A vendor interface for identity commands.
//! Sends only read-only queries documented by the armorx-toolkit research:
//! GetDevicePowerInfo(04), GetZkmVersion(0B), GetUUID(EF), GetMode(E2),
//! GetProfileSize(D3), GetMacroList(D5), ReadConfig(D6).
//!
//! Wire format: 65-byte Windows HID buffers, report id 0; payload frames are
//! `A5 | length(incl. A5 and checksum) | opcode | data | checksum`, checksum
//! being the wrapping sum of every preceding frame byte. Long payloads come
//! back as `A4 | length | opcode | index | data | checksum` fragments.
use hidapi::HidApi;
use std::time::{Duration, Instant};

const VID: u16 = 0x413d;
const USAGE_PAGE: u16 = 0xff7a;
const USAGE: u16 = 1;

fn short(opcode: u8) -> [u8; 65] {
    let mut report = [0u8; 65];
    report[1] = 0xa5;
    report[2] = 0x04;
    report[3] = opcode;
    report[4] = report[1..4].iter().fold(0u8, |sum, b| sum.wrapping_add(*b));
    report
}

/// Copy of the official client's request: A5 0C EF followed by eight zero
/// bytes (slot selector) and the checksum.
fn get_uuid() -> [u8; 65] {
    let mut report = [0u8; 65];
    report[1] = 0xa5;
    report[2] = 0x0c;
    report[3] = 0xef;
    report[12] = report[1..12]
        .iter()
        .fold(0u8, |sum, b| sum.wrapping_add(*b));
    report
}

fn checksum_ok(frame: &[u8]) -> bool {
    let total = frame[1] as usize;
    total >= 4
        && total <= frame.len()
        && frame[..total - 1]
            .iter()
            .fold(0u8, |sum, b| sum.wrapping_add(*b))
            == frame[total - 1]
}

fn dump(label: &str, frame: &[u8]) {
    if frame[0] != 0xa5 && frame[0] != 0xa4 {
        return;
    }
    let total = (frame[1] as usize).min(frame.len()).max(1);
    let hex = frame[..total]
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ");
    println!("{label} [{}] {hex}", if checksum_ok(frame) { "校验通过" } else { "校验存疑" });
}

fn main() {
    let api = HidApi::new().expect("hidapi 初始化失败");
    let mut handles = None;
    for info in api.device_list() {
        if info.vendor_id() == VID && info.usage_page() == USAGE_PAGE && info.usage() == USAGE {
            println!(
                "打开接口: {} ({})",
                info.path().to_string_lossy(),
                info.product_string().unwrap_or("无名")
            );
            // Two handles, mirroring open_pair: never mix an outstanding read
            // with writes on the same handle.
            let reader = api.open_path(info.path()).expect("打开 FF7A 读句柄失败");
            let writer = api.open_path(info.path()).expect("打开 FF7A 写句柄失败");
            handles = Some((reader, writer));
            break;
        }
    }
    let Some((reader, writer)) = handles else {
        println!("未找到 VID 413D 的 FF7A 接口，请确认手柄已通过接收器连接");
        return;
    };

    let mut buffer = [0u8; 256];
    let probes: [(&str, [u8; 65]); 7] = [
        ("GetDevicePowerInfo(04)", short(0x04)),
        ("GetZkmVersion(0B)", short(0x0b)),
        ("GetUUID(EF)", get_uuid()),
        ("GetMode(E2)", short(0xe2)),
        ("GetProfileSize(D3)", short(0xd3)),
        ("GetMacroList(D5)", short(0xd5)),
        ("ReadConfig(D6)", short(0xd6)),
    ];
    for (label, report) in probes {
        let opcode = report[3];
        // Clear stale input so the dump only holds this query's answers.
        while matches!(reader.read_timeout(&mut buffer, 8), Ok(n) if n > 0) {}
        // Same fallback as the app's send(): HidD_SetOutputReport rejects the
        // buffer on this interface (os error 1784), WriteFile accepts it.
        let sent = writer
            .send_output_report(&report)
            .or_else(|_| writer.write(&report).map(|_| ()));
        if let Err(error) = sent {
            println!("{label} 发送失败: {error}");
            continue;
        }
        let deadline = Instant::now() + Duration::from_millis(400);
        let mut answered = false;
        while Instant::now() < deadline {
            match reader.read_timeout(&mut buffer, 60) {
                Ok(n) if n > 0 => {
                    let opcode_echo = buffer.get(2).copied();
                    if opcode_echo == Some(opcode) || buffer[0] == 0xa4 {
                        dump(label, &buffer[..n]);
                        answered = true;
                    } else if buffer[0] == 0xa5 {
                        dump(&format!("{label} (其他帧)"), &buffer[..n]);
                    }
                }
                _ => continue,
            }
        }
        if !answered {
            println!("{label} 无应答");
        }
        println!("---");
    }
}
