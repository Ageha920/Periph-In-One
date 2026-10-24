use periph_in_one::{
    config::Config,
    model::{self, Reading, SavedDevice},
    protocol::*,
};

fn reading(physical: &str, serial: &str, battery: Option<u8>) -> Reading {
    Reading {
        physical_id: physical.into(),
        kind: "mouse".into(),
        hardware_name: "Receiver".into(),
        serial: serial.into(),
        vid: 0x373b,
        pid: 0x1216,
        battery,
        message: String::new(),
    }
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
fn native_hidapi_zero_report_id_is_restored() {
    let frame = darkrim_query(7, 1);
    assert_eq!(restore_darkrim_report_id(&frame[1..]), frame);
    assert_eq!(restore_darkrim_report_id(&frame), frame);
    assert!(darkrim_tlvs(&restore_darkrim_report_id(&frame[1..]), 7).is_some());
}

#[test]
fn mouse_requires_correct_reply_and_valid_percentage() {
    let mut reply = [0; 64];
    reply[1] = 0x72;
    reply[5] = 7;
    reply[7] = 0;
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
    let saved = vec![SavedDevice {
        id: model::serial_key(0x373b, 0x1216, "CLONED"),
        serial: "CLONED".into(),
        vid: 0x373b,
        pid: 0x1216,
        alias: "私人鼠标".into(),
        ..Default::default()
    }];
    let devices = model::merge_readings(
        &saved,
        vec![
            reading("a", "CLONED", Some(50)),
            reading("b", "CLONED", Some(60)),
        ],
        100,
    );
    assert_eq!(devices.len(), 3);
    assert!(devices[0].saved.identity_conflict);
    assert!(
        devices
            .iter()
            .filter(|d| d.present)
            .all(|d| d.saved.alias.is_empty())
    );
    let saved: Vec<_> = devices.iter().map(|d| d.saved.clone()).collect();
    let later = model::merge_readings(&saved, vec![reading("c", "CLONED", Some(70))], 200);
    assert!(
        later
            .iter()
            .filter(|d| d.present)
            .all(|d| d.saved.alias.is_empty())
    );
}

#[test]
fn absent_serial_does_not_merge_same_model_receivers() {
    let devices = model::merge_readings(
        &[],
        vec![reading("a", "", None), reading("b", "", None)],
        100,
    );
    assert_eq!(devices.len(), 2);
    assert!(devices.iter().all(|d| d.saved.serial.is_empty()));
}

#[test]
fn weak_id_cannot_reuse_remarks_after_restart_or_observed_disconnect() {
    let mut tracker = model::SessionTracker::new("run-a".into());
    let first = model::merge_readings(
        &[],
        tracker.track(vec![reading("same-port", "", Some(50))]),
        1,
    );
    let mut saved: Vec<_> = first.iter().map(|d| d.saved.clone()).collect();
    saved[0].alias = "旧接收器".into();
    let continuous = model::merge_readings(
        &saved,
        tracker.track(vec![reading("same-port", "", Some(51))]),
        2,
    );
    assert_eq!(continuous.len(), 1);
    tracker.track(vec![]);
    let reconnected = model::merge_readings(
        &saved,
        tracker.track(vec![reading("same-port", "", Some(52))]),
        3,
    );
    assert!(
        reconnected
            .iter()
            .filter(|d| d.present)
            .all(|d| d.saved.alias.is_empty())
    );
    let mut restarted = model::SessionTracker::new("run-b".into());
    let after_restart = model::merge_readings(
        &saved,
        restarted.track(vec![reading("same-port", "", Some(53))]),
        4,
    );
    assert!(
        after_restart
            .iter()
            .filter(|d| d.present)
            .all(|d| d.saved.alias.is_empty())
    );
}

#[test]
fn product_name_is_not_a_reliable_unique_serial() {
    let devices = model::merge_readings(&[], vec![reading("port", "Receiver", Some(35))], 1);
    assert!(devices[0].saved.identity_conflict);
    assert!(devices[0].saved.id.starts_with("local:"));
    assert_eq!(devices[0].saved.serial, "Receiver");
}

#[test]
fn rebind_moves_a_remark_only_after_explicit_selection() {
    let mut devices = model::merge_readings(&[], vec![reading("a", "A", None)], 1);
    devices[0].saved.alias = "保留的备注".into();
    let saved: Vec<_> = devices.iter().map(|d| d.saved.clone()).collect();
    let mut devices = model::merge_readings(&saved, vec![reading("b", "B", None)], 2);
    let from = devices[0].saved.id.clone();
    let to = devices[1].saved.id.clone();
    assert!(model::rebind(&mut devices, &from, &from).is_err());
    model::rebind(&mut devices, &from, &to).unwrap();
    assert!(devices[0].saved.alias.is_empty());
    assert_eq!(devices[1].saved.alias, "保留的备注");
    assert!(model::rebind(&mut devices, &to, &from).is_err());
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
