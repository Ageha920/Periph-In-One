use battery_island::{config::Config, model::{self, Reading, SavedDevice}, protocol::*};

fn reading(physical: &str, serial: &str, battery: Option<u8>) -> Reading {
    Reading { physical_id: physical.into(), kind: "mouse".into(), hardware_name: "Receiver".into(), serial: serial.into(), vid: 0x373b, pid: 0x1216, battery, message: String::new() }
}

#[test]
fn crc_reference_and_corrupt_frames() {
    assert_eq!(crc16(b"123456789"), 0x29b1);
    let mut report = darkrim_query(7, 1);
    report[9] = 0;
    assert_eq!(darkrim_tlvs(&report, 7).unwrap(), vec![(1, &[][..])]);
    assert!(darkrim_tlvs(&report, 8).is_none());
    report[11] ^= 1;
    assert!(darkrim_tlvs(&report, 7).is_none());
    assert!(darkrim_tlvs(&report[..10], 7).is_none());
}

#[test]
fn rejects_malformed_tlv_even_with_valid_crc() {
    let mut report = darkrim_query(1, 1);
    report[11] = 4;
    let crc = crc16(&report[1..12]).to_be_bytes();
    report[12..14].copy_from_slice(&crc);
    assert!(darkrim_tlvs(&report, 1).is_none());
}

#[test]
fn mouse_requires_correct_reply_and_valid_percentage() {
    let mut reply = [0; 64];
    reply[1] = 0x72; reply[5] = 7; reply[7] = 0;
    assert_eq!(atk_battery(&reply), Some(0));
    reply[7] = 101;
    assert_eq!(atk_battery(&reply), None);
    assert_eq!(atk_battery(&[]), None);
}

#[test]
fn remarks_follow_serial_across_ports_and_stale_values_are_retained() {
    let first = model::merge_readings(&[], vec![reading("usb-port-a", "SN-A", Some(73))], 100);
    let mut saved: Vec<_> = first.iter().map(|d| d.saved.clone()).collect();
    saved[0].alias = "办公鼠标".into();
    let second = model::merge_readings(&saved, vec![reading("usb-port-b", "SN-A", None)], 200);
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].saved.alias, "办公鼠标");
    assert_eq!(second[0].saved.battery, Some(73));
    assert_eq!(second[0].saved.last_success, Some(100));
    assert!(!second[0].fresh);
    let offline = model::merge_readings(&saved, vec![], 300);
    assert!(!offline[0].present);
}

#[test]
fn duplicate_serials_cannot_inherit_a_historical_alias() {
    let saved = vec![SavedDevice { id: model::serial_key(0x373b, 0x1216, "CLONED"), serial: "CLONED".into(), vid: 0x373b, pid: 0x1216, alias: "私人鼠标".into(), ..Default::default() }];
    let devices = model::merge_readings(&saved, vec![reading("a", "CLONED", Some(50)), reading("b", "CLONED", Some(60))], 100);
    assert_eq!(devices.len(), 3);
    assert!(devices[0].saved.identity_conflict);
    assert!(devices.iter().filter(|d| d.present).all(|d| d.saved.alias.is_empty()));
    let saved: Vec<_> = devices.iter().map(|d| d.saved.clone()).collect();
    let later = model::merge_readings(&saved, vec![reading("c", "CLONED", Some(70))], 200);
    assert!(later.iter().filter(|d| d.present).all(|d| d.saved.alias.is_empty()));
}

#[test]
fn absent_serial_does_not_merge_same_model_receivers() {
    let devices = model::merge_readings(&[], vec![reading("a", "", None), reading("b", "", None)], 100);
    assert_eq!(devices.len(), 2);
    assert!(devices.iter().all(|d| d.saved.serial.is_empty()));
}

#[test]
fn config_roundtrip_atomic_replacement_and_bad_file_detection() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config/settings.json");
    let mut config = Config::load(&path).unwrap();
    config.width = 900.0;
    config.save(&path).unwrap();
    config.height = 400.0;
    config.save(&path).unwrap();
    let loaded = Config::load(&path).unwrap();
    assert_eq!((loaded.width, loaded.height), (900.0, 400.0));
    std::fs::write(&path, b"{broken").unwrap();
    assert!(Config::load(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"{broken");
}
