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
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
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

/// Reject truncated TLVs, incorrect sequence IDs and bad CRCs before using data.
pub fn darkrim_tlvs(report: &[u8], expected_seq: u8) -> Option<Vec<(u8, &[u8])>> {
    if report.len() < 12 || report[0] != 0 || report[1] != 0xfe || report[2] != expected_seq {
        return None;
    }
    let end = 10 + u16::from_le_bytes([report[8], report[9]]) as usize;
    if end + 2 > report.len() || crc16(&report[1..end]) != u16::from_be_bytes([report[end], report[end + 1]]) {
        return None;
    }
    let mut tlvs = Vec::new();
    let mut i = 10;
    while i < end {
        if i + 2 > end { return None; }
        let next = i + 2 + report[i + 1] as usize;
        if next > end { return None; }
        tlvs.push((report[i], &report[i + 2..next]));
        i = next;
    }
    Some(tlvs)
}

