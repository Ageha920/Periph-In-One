//! Probe the Zikway/BIGBIG WON FF7A vendor interface for identity commands.
//! Sends only read-only queries: GetDevicePowerInfo(04), GetZkmVersion(0B),
//! GetUUID(EF), GetMode(E2), GetProfileSize(D3), GetMacroList(D5) and
//! ReadConfig(D6) from the armorx-toolkit research; GetWirelessFlag(FA) from
//! the DevMgr.dll static analysis (reply A5 05 FA <flag> <sum>), not yet
//! verified live.
//!
//! Wire format: 65-byte Windows HID buffers, report id 0; payload frames are
//! `A5 | length(incl. A5 and checksum) | opcode | data | checksum`, checksum
//! being the wrapping sum of every preceding frame byte. Long payloads come
//! back as `A4 | length | opcode | index | data | checksum` fragments.
use hidapi::HidApi;
use periph_in_one::protocol;
use std::time::{Duration, Instant};

const VID: u16 = 0x413d;
const USAGE_PAGE: u16 = 0xff7a;
const USAGE: u16 = 1;

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
    println!(
        "{label} [{}] {hex}",
        if checksum_ok(frame) {
            "校验通过"
        } else {
            "校验存疑"
        }
    );
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
    // Frames come from the app's protocol module; GetUUID mirrors the
    // official client's A5 0C EF + eight zero selector bytes request.
    let probes: [(&str, [u8; 65]); 8] = [
        (
            "GetDevicePowerInfo(04)",
            protocol::bigbigwon_short_request(0x04),
        ),
        ("GetZkmVersion(0B)", protocol::bigbigwon_short_request(0x0b)),
        ("GetUUID(EF)", protocol::bigbigwon_uuid_request()),
        ("GetMode(E2)", protocol::bigbigwon_short_request(0xe2)),
        (
            "GetProfileSize(D3)",
            protocol::bigbigwon_short_request(0xd3),
        ),
        ("GetMacroList(D5)", protocol::bigbigwon_short_request(0xd5)),
        ("ReadConfig(D6)", protocol::bigbigwon_short_request(0xd6)),
        (
            "GetWirelessFlag(FA)",
            protocol::bigbigwon_short_request(0xfa),
        ),
    ];
    let mut answered_total = 0;
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
                        answered_total += 1;
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

    // The idle personality keeps the interrupt endpoint silent, but the
    // control path may still answer GetFeature reads. Probe the same
    // read-only set through feature reports before declaring it mute.
    if answered_total == 0 {
        println!("== feature 通道试探 ==");
        for (label, report) in probes {
            let mut request = report;
            request[0] = 0;
            match reader.get_feature_report(&mut request) {
                Ok(n) if n > 1 => {
                    let hex = request[..n.min(24)]
                        .iter()
                        .map(|b| format!("{b:02X}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    println!("{label} feature {n} 字节: {hex}");
                }
                Ok(_) => println!("{label} feature 空应答"),
                Err(error) => println!("{label} feature 失败: {error}"),
            }
        }
    }
}
