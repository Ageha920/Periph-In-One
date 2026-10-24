//! Wire formats ported from the verified Python readers. No device I/O here.

pub fn atk_request() -> [u8; 64] {
    let mut report = [0; 64];
    report[..8].copy_from_slice(&[0x08, 0x7d, 0x72, 0x02, 0x00, 0x01, 0x07, 0x01]);
    report
}

pub fn atk_battery(report: &[u8]) -> Option<u8> {
    (report.len() >= 8 && report[1] == 0x72 && report[5] == 0x07 && report[7] <= 100)
        .then(|| report[7])
}

pub fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0xffff_u16;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

pub fn darkrim_query(seq: u8, kind: u8) -> [u8; 65] {
    let mut report = [0; 65];
    report[..12].copy_from_slice(&[0, 0xfe, seq, 0x10, 0, 2, 0, 0, 2, 0, kind, 0]);
    let crc = crc16(&report[1..12]).to_be_bytes();
    report[12..14].copy_from_slice(&crc);
    report
}

/// hidapi's native Windows backend removes the zero report-ID byte on input.
/// The original ctypes reader and our wire parser include that byte.
pub fn restore_darkrim_report_id(input: &[u8]) -> Vec<u8> {
    if input.first() == Some(&0xfe) {
        std::iter::once(0).chain(input.iter().copied()).collect()
    } else {
        input.to_vec()
    }
}

/// Reject truncated TLVs, incorrect sequence IDs and bad CRCs before using data.
pub fn darkrim_tlvs(report: &[u8], expected_seq: u8) -> Option<Vec<(u8, &[u8])>> {
    if report.len() < 12 || report[0] != 0 || report[1] != 0xfe || report[2] != expected_seq {
        return None;
    }
    let end = 10 + u16::from_le_bytes([report[8], report[9]]) as usize;
    if end + 2 > report.len()
        || crc16(&report[1..end]) != u16::from_be_bytes([report[end], report[end + 1]])
    {
        return None;
    }
    let mut tlvs = Vec::new();
    let mut i = 10;
    while i < end {
        if i + 2 > end {
            return None;
        }
        let next = i + 2 + report[i + 1] as usize;
        if next > end {
            return None;
        }
        tlvs.push((report[i], &report[i + 2..next]));
        i = next;
    }
    Some(tlvs)
}

/// KZZI/Compx 2.4G receiver frames: report id 0x13, 20 bytes, the last byte
/// being the sum of the previous 19. The battery layout follows the vendor
/// driver captures mirrored by womier-l65-linux and sanshoku issue 36.
pub fn kzzi_request(command: u8) -> [u8; 20] {
    let mut frame = [0; 20];
    frame[0] = 0x13;
    frame[1] = command;
    frame[19] = frame[..19]
        .iter()
        .fold(0u8, |sum, &byte| sum.wrapping_add(byte));
    frame
}

/// The identity request answers with npkg=1 in byte 2 on this receiver.
pub fn kzzi_identity_request() -> [u8; 20] {
    let mut frame = kzzi_request(0x05);
    frame[2] = 1;
    frame[19] = frame[..19]
        .iter()
        .fold(0u8, |sum, &byte| sum.wrapping_add(byte));
    frame
}

fn kzzi_frame_ok(report: &[u8]) -> bool {
    report.len() == 20
        && report[0] == 0x13
        && report[..19]
            .iter()
            .fold(0u8, |sum, &byte| sum.wrapping_add(byte))
            == report[19]
}

/// Identity reply carries the vendor profile signature (PSD) in bytes 5..11.
pub fn kzzi_identity(report: &[u8]) -> Option<[u8; 6]> {
    if !kzzi_frame_ok(report) || report[1] != 0x05 {
        return None;
    }
    let mut psd = [0; 6];
    psd.copy_from_slice(&report[5..11]);
    Some(psd)
}

/// Battery reply: (level, state); 0x01 on battery, 0x10 cable/charging.
/// The receiver streams unrelated 0x13 frames, so the command nibble, the
/// checksum and the 1..=100 level range all gate a reading.
pub fn kzzi_battery(report: &[u8]) -> Option<(u8, u8)> {
    if !kzzi_frame_ok(report) || report[1] & 0x7f != 0x4a {
        return None;
    }
    let (level, state) = (report[5], report[6]);
    ((1..=100).contains(&level)).then_some((level, state))
}

/// Model names keyed by the profile signature recorded in the vendor driver's
/// device profiles (Dev/*/KB.ini Psd= hex values, names from Cfg.ini DevName).
pub fn kzzi_model_name(psd: &[u8]) -> Option<&'static str> {
    Some(match psd {
        [2, 0, 0, 0, 0, 54] => "Kzzi-K75 RGB",
        [2, 0, 0, 0, 0, 55] => "Kzzi-K75 RGB key",
        [3, 0, 0, 0, 0, 2] => "Kzzi-K68",
        [3, 0, 0, 0, 0, 4] => "Kzzi-K84",
        [3, 0, 0, 0, 0, 103] => "Kzzi-K75Pro",
        [3, 0, 0, 0, 0, 224] => "Kzzi-K20RGB",
        [3, 0, 0, 0, 0, 252] => "Kzzi-K68Pro",
        [3, 0, 0, 0, 1, 10] => "Kzzi-K98",
        [3, 0, 0, 0, 1, 52] => "Kzzi-K87",
        [3, 0, 0, 0, 1, 68] => "KZZI-K75RGB Lite",
        [3, 0, 0, 0, 1, 104] => "Kzzi-i75 RGB",
        [3, 0, 0, 0, 1, 215] => "Kzzi-K87MAX",
        [3, 0, 0, 0, 3, 162] => "Kzzi-K75V2",
        [3, 0, 0, 0, 3, 187] => "Kzzi-Z75",
        [3, 0, 0, 0, 3, 215] => "Kzzi K98V2",
        [3, 0, 0, 0, 3, 243] => "Kzzi-i98",
        [3, 0, 0, 0, 3, 247] => "Kzzi-i20",
        [3, 0, 0, 0, 5, 62] => "Kzzi-i98",
        [6, 0, 0, 0, 0, 1] => "Kzzi-Z98",
        [6, 0, 0, 0, 0, 29] => "Kzzi-K21",
        _ => return None,
    })
}
