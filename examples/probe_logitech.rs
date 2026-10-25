//! Probe a Logitech Unifying-family receiver for the routes its registers
//! answer on Windows, where the two FF00 vendor collections surface as
//! separate hidapi devices (short 0x10 frames on usage 0x01, long 0x11
//! frames on usage 0x02). Findings recorded on a G502 LIGHTSPEED behind
//! C539 (2026-10-25); read-only — every frame below is a register read or
//! a device feature lookup.
//!
//! Observed on C539:
//! - Short register reads (0x81) answer instantly on the 0x01 collection;
//!   the connections register (0x02) reports the PAIRED slot count, so a
//!   paired but asleep mouse is still swept. Long frames are refused by
//!   Windows on the 0x01 collection and short frames on the 0x02 one.
//! - Long register reads go out as 0x83 (0x8100 | 0x2B5 — the 0x200 bit of
//!   the long register value carries into the sub id, so 0xB5 reads are
//!   0x83, NOT 0x82; 0x82 is the long WRITE). The pairing sub-registers
//!   (0x20/0x30) never answer on this receiver; the receiver information
//!   sub-register (0x03) answers `11 FF 83 B5 03 <serial 4> <maxdev> …` but
//!   seconds to minutes late and out of order across runs — both reasons
//!   the scan adapter leaves the long registers alone.
//! - Device-tunnelled HID++ 2.0 answers on the 0x02 collection once the
//!   mouse is awake; a waking device replies one exchange window late
//!   (~0.5–1 s), so short read windows miss it.
use hidapi::{HidApi, HidDevice};
use std::time::{Duration, Instant};

const VID: u16 = 0x046d;
const LIGHTSPEED_C539: u16 = 0xc539;
const UNIFYING_C52B: u16 = 0xc52b;
const BOLT_C548: u16 = 0xc548;

fn dump(label: &str, frame: &[u8]) {
    let hex = frame
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ");
    println!("{label} {hex}");
}

/// Send one frame, print every answer arriving within the window.
fn exchange(label: &str, reader: &HidDevice, writer: &HidDevice, frame: &[u8], window: Duration) {
    let mut buffer = [0u8; 64];
    while reader.read_timeout(&mut buffer, 30).unwrap_or(0) > 0 {}
    if writer.write(frame).is_err() {
        println!("{label} 写入失败");
        return;
    }
    let end = Instant::now() + window;
    let mut answered = false;
    while Instant::now() < end {
        match reader.read_timeout(&mut buffer, 80) {
            Ok(n) if n > 0 => {
                dump(label, &buffer[..n]);
                answered = true;
            }
            _ => continue,
        }
    }
    if !answered {
        println!("{label} 无应答");
    }
}

fn main() {
    let api = HidApi::new().expect("hidapi");
    let mut found = Vec::new();
    for info in api.device_list() {
        if info.vendor_id() == VID
            && matches!(
                info.product_id(),
                LIGHTSPEED_C539 | UNIFYING_C52B | BOLT_C548
            )
            && info.usage_page() == 0xff00
            && matches!(info.usage(), 0x01 | 0x02)
        {
            found.push((info.product_id(), info.usage(), info.path().to_owned()));
        }
    }
    if found.is_empty() {
        println!("未发现罗技接收器（C539/C52B/C548）");
        return;
    }
    let (pid, _, _) = found[0].clone();
    let Some((_, _, short_path)) = found.iter().find(|(_, usage, _)| *usage == 0x01) else {
        return;
    };
    let Some((_, _, long_path)) = found.iter().find(|(_, usage, _)| *usage == 0x02) else {
        return;
    };
    println!("接收器 {pid:04X}，共 {} 个 vendor 接口", found.len());
    let short_device = api.open_path(short_path).expect("打开短帧接口");
    let long_device = api.open_path(long_path).expect("打开长帧接口");
    // Baseline: the short register read the scan adapter relies on.
    let register = [0x10, 0xff, 0x81, 0x02, 0x00, 0x00, 0x00];
    exchange(
        "短寄存器 0x81 配对数(0x02) ",
        &short_device,
        &short_device,
        &register,
        Duration::from_millis(500),
    );
    // Pairing information, slot 1: unifying sub 0x20; Bolt would be 0x51.
    // Long read = 0x83 (see the module notes).
    let pairing_long = [
        0x11, 0xff, 0x83, 0xb5, 0x20, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    exchange(
        "长寄存器 0x83 配对信息     ",
        &long_device,
        &long_device,
        &pairing_long,
        Duration::from_secs(4),
    );
    // Extended pairing information (unit serial), unifying sub 0x30.
    let extended_long = [
        0x11, 0xff, 0x83, 0xb5, 0x30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    exchange(
        "长寄存器 0x83 扩展配对     ",
        &long_device,
        &long_device,
        &extended_long,
        Duration::from_secs(4),
    );
    // Receiver information: serial + max devices.
    let info_long = [
        0x11, 0xff, 0x83, 0xb5, 0x03, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    exchange(
        "长寄存器 0x83 接收器信息   ",
        &long_device,
        &long_device,
        &info_long,
        Duration::from_secs(6),
    );
    // Device-tunnelled HID++ 2.0 for the paired mouse: ping, then the
    // feature lookups the adapter tries in order.
    let ping = [0x11, 0x01, 0x00, 0x1b, 0x00, 0x00, 0x42];
    exchange(
        "隧道 Ping 槽位1           ",
        &long_device,
        &long_device,
        &ping,
        Duration::from_secs(2),
    );
    for (label, feature) in [
        ("隧道 GetFeature 0x0003    ", [0x00, 0x03u8]),
        ("隧道 GetFeature 0x1004    ", [0x10, 0x04]),
        ("隧道 GetFeature 0x1000    ", [0x10, 0x00]),
        ("隧道 GetFeature 0x1001    ", [0x10, 0x01]),
    ] {
        let call = [
            0x11, 0x01, 0x00, 0x0b, feature[0], feature[1], 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0,
        ];
        exchange(
            label,
            &long_device,
            &long_device,
            &call,
            Duration::from_secs(2),
        );
    }
}
