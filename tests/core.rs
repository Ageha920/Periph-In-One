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
        uid: String::new(),
        vid: 0x373b,
        pid: 0x1216,
        battery,
        message: String::new(),
    }
}

fn gamepad_reading(physical: &str, uid: &str, pid: u16, battery: Option<u8>) -> Reading {
    Reading {
        physical_id: physical.into(),
        kind: "gamepad".into(),
        hardware_name: if uid.is_empty() {
            "HID zkm".into()
        } else {
            "XBOX 360 For Windows".into()
        },
        serial: String::new(),
        uid: uid.into(),
        vid: 0x413d,
        pid,
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
fn kzzi_requests_carry_the_receiver_checksum() {
    let battery = kzzi_request(0x4a);
    assert_eq!(&battery[..4], &[0x13, 0x4a, 0, 0]);
    assert_eq!(battery[19], 0x5d);
    let identity = kzzi_identity_request();
    assert_eq!(&identity[..4], &[0x13, 0x05, 1, 0]);
    assert_eq!(identity[19], 0x19);
    // A battery request parses as a zero level, never as a reading.
    assert_eq!(kzzi_battery(&battery), None);
}

#[test]
fn kzzi_battery_reply_requires_checksum_command_and_level_range() {
    let mut reply =
        *b"\x13\x4a\x01\x00\x02\x61\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\xc2";
    assert_eq!(kzzi_battery(&reply), Some((0x61, 0x01)));
    reply[5] = 100;
    reply[6] = 0x10;
    reply[19] = reply[..19].iter().fold(0u8, |s, &b| s.wrapping_add(b));
    assert_eq!(kzzi_battery(&reply), Some((100, 0x10)));
    for level in [0u8, 101] {
        reply[5] = level;
        reply[19] = reply[..19].iter().fold(0u8, |s, &b| s.wrapping_add(b));
        assert_eq!(kzzi_battery(&reply), None);
    }
    reply[19] ^= 1;
    assert_eq!(kzzi_battery(&reply), None);
    assert_eq!(kzzi_battery(&reply[..12]), None);
    assert_eq!(kzzi_battery(&[]), None);
    // Unrelated frames the receiver also streams must be ignored.
    let mut status =
        *b"\x13\x0a\x01\x00\x02\x61\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x9b";
    status[19] = status[..19].iter().fold(0u8, |s, &b| s.wrapping_add(b));
    assert_eq!(kzzi_battery(&status), None);
}

#[test]
fn kzzi_identity_reply_yields_the_model_signature() {
    let reply =
        *b"\x13\x05\x01\x00\x0a\x02\x00\x00\x00\x00\x37\x00\x01\x00\x00\x00\x00\x00\x00\x5d";
    assert_eq!(kzzi_identity(&reply), Some([2, 0, 0, 0, 0, 55]));
    assert_eq!(
        kzzi_model_name(&[2, 0, 0, 0, 0, 55]),
        Some("Kzzi-K75 RGB key")
    );
    assert_eq!(kzzi_model_name(&[2, 0, 0, 0, 0, 54]), Some("Kzzi-K75 RGB"));
    assert_eq!(kzzi_model_name(&[9, 9, 9, 9, 9, 9]), None);
    let mut corrupt = reply;
    corrupt[19] ^= 1;
    assert_eq!(kzzi_identity(&corrupt), None);
    assert_eq!(kzzi_identity(&reply[..9]), None);
    // A battery reply is not an identity reply.
    assert_eq!(kzzi_identity(kzzi_request(0x4a).as_slice()), None);
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
fn local_identity_and_remark_survive_restart_and_reconnect() {
    let first = model::merge_readings(&[], vec![reading("same-port", "", Some(50))], 1);
    let mut saved: Vec<_> = first.iter().map(|d| d.saved.clone()).collect();
    saved[0].alias = "办公鼠标".into();
    // Serialize/load just as two separate processes do.
    let saved: Vec<SavedDevice> =
        serde_json::from_str(&serde_json::to_string(&saved).unwrap()).unwrap();
    let offline = model::merge_readings(&saved, vec![], 2);
    let saved: Vec<_> = offline.into_iter().map(|d| d.saved).collect();
    let restarted = model::merge_readings(&saved, vec![reading("same-port", "", Some(53))], 3);
    assert_eq!(restarted.len(), 1);
    assert!(restarted[0].present);
    assert_eq!(restarted[0].saved.alias, "办公鼠标");
    assert_eq!(restarted[0].saved.battery, Some(53));
}

fn legacy_record(session: &str, physical: &str, alias: &str, time: u64) -> SavedDevice {
    SavedDevice {
        id: format!(
            "local:{}",
            serde_json::to_string(&(0x373b, 0x1216, format!("{session}:1:{physical}"))).unwrap()
        ),
        vid: 0x373b,
        pid: 0x1216,
        serial: "Receiver".into(),
        hardware_name: "Receiver".into(),
        alias: alias.into(),
        last_success: Some(time),
        battery: Some(42),
        identity_conflict: true,
        ..Default::default()
    }
}

#[test]
fn migrates_repeated_launch_records_without_losing_remark_or_last_reading() {
    let saved = vec![
        legacy_record("123-1700000000", "container-a", "办公鼠标", 10),
        legacy_record("456-1800000000", "container-a", "", 20),
        legacy_record("789-1900000000", "container-b", "另一台", 30),
    ];
    let devices = model::merge_readings(&saved, vec![reading("container-a", "Receiver", None)], 40);
    assert_eq!(devices.len(), 2);
    let active = devices.iter().find(|d| d.present).unwrap();
    assert_eq!(active.saved.alias, "办公鼠标");
    assert_eq!(active.saved.last_success, Some(20));
    assert_eq!(active.saved.battery, Some(42));
    assert_eq!(active.saved.physical_id, "container-a");
    let saved: Vec<_> = devices.into_iter().map(|d| d.saved).collect();
    let later = model::merge_readings(
        &saved,
        vec![reading("container-a", "Receiver", Some(50))],
        50,
    );
    assert_eq!(later.len(), 2);
    assert_eq!(later.iter().filter(|d| d.present).count(), 1);
}

#[test]
fn conflicting_legacy_remarks_are_preserved_for_manual_review() {
    let saved = vec![
        legacy_record("1-100", "a", "甲", 1),
        legacy_record("2-200", "a", "乙", 2),
    ];
    let devices = model::merge_readings(&saved, vec![reading("a", "Receiver", Some(50))], 3);
    assert_eq!(devices.len(), 2);
    assert!(devices.iter().any(|d| d.saved.alias == "甲"));
    assert!(devices.iter().any(|d| d.saved.alias == "乙"));
}

#[test]
fn offline_deletion_does_not_allow_present_or_nonresponding_devices() {
    let mut devices = model::merge_readings(&[], vec![reading("a", "A", None)], 1);
    let id = devices[0].saved.id.clone();
    assert!(model::remove_offline(&mut devices, &id).is_err());
    let saved: Vec<_> = devices.into_iter().map(|d| d.saved).collect();
    let mut devices = model::merge_readings(&saved, vec![], 2);
    model::remove_offline(&mut devices, &id).unwrap();
    assert!(devices.is_empty());
    assert!(model::remove_offline(&mut devices, &id).is_err());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config/settings.json");
    Config {
        devices: devices.into_iter().map(|d| d.saved).collect(),
        ..Config::default()
    }
    .save(&path)
    .unwrap();
    let restored = Config::load(&path).unwrap();
    assert!(model::merge_readings(&restored.devices, vec![], 3).is_empty());
}

#[test]
fn temporarily_missing_descriptor_does_not_create_a_duplicate() {
    let first = model::merge_readings(&[], vec![reading("a", "SN-A", Some(70))], 1);
    let saved: Vec<_> = first.into_iter().map(|d| d.saved).collect();
    let unavailable = model::merge_readings(&saved, vec![reading("a", "", None)], 2);
    assert_eq!(unavailable.len(), 1);
    assert_eq!(unavailable[0].saved.serial, "SN-A");
    let saved: Vec<_> = unavailable.into_iter().map(|d| d.saved).collect();
    let restored = model::merge_readings(&saved, vec![reading("a", "SN-A", Some(71))], 3);
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].saved.battery, Some(71));
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

#[test]
fn bigbigwon_request_frame_matches_verified_wiring() {
    let report = bigbigwon_request();
    assert_eq!(report.len(), 65);
    assert_eq!(&report[..5], &[0x00, 0xa5, 0x04, 0x04, 0xad]);
    assert!(report[5..].iter().all(|&byte| byte == 0));
}

#[test]
fn bigbigwon_battery_parses_verified_power_reply() {
    // Captured replies: 88%/90% alternation from a near-full GALE controller.
    let reply = [0xa5, 0x06, 0x04, 0x58, 0x00, 0x07];
    assert_eq!(bigbigwon_battery(&reply), Some(88));
    let reply = [0xa5, 0x06, 0x04, 0x5a, 0x00, 0x09];
    assert_eq!(bigbigwon_battery(&reply), Some(90));
    // The trailing zeros of the 64-byte hidapi report must not disturb parsing.
    let mut padded = [0u8; 64];
    padded[..6].copy_from_slice(&[0xa5, 0x06, 0x04, 0x58, 0x00, 0x07]);
    assert_eq!(bigbigwon_battery(&padded), Some(88));
}

#[test]
fn bigbigwon_battery_rejects_bad_frames() {
    // Broken additive checksum.
    assert_eq!(
        bigbigwon_battery(&[0xa5, 0x06, 0x04, 0x58, 0x00, 0x08]),
        None
    );
    // Wrong command echo.
    assert_eq!(
        bigbigwon_battery(&[0xa5, 0x06, 0x0b, 0x36, 0xeb, 0x00]),
        None
    );
    // Level beyond 100%.
    assert_eq!(
        bigbigwon_battery(&[0xa5, 0x06, 0x04, 0xc8, 0x00, 0x77]),
        None
    );
    // Truncated and wrong-magic frames.
    assert_eq!(bigbigwon_battery(&[0xa5, 0x06, 0x04]), None);
    assert_eq!(
        bigbigwon_battery(&[0x13, 0x06, 0x04, 0x58, 0x00, 0x07]),
        None
    );
    // Length byte larger than the buffer.
    assert_eq!(bigbigwon_battery(&[0xa5, 0xff, 0x04, 0x58]), None);
    // Zero length byte must not underflow.
    assert_eq!(bigbigwon_battery(&[0xa5, 0x00]), None);
}

#[test]
fn bigbigwon_uuid_request_mirrors_the_official_client() {
    let request = bigbigwon_uuid_request();
    assert_eq!(request.len(), 65);
    assert_eq!(&request[1..4], &[0xa5, 0x0c, 0xef]);
    assert_eq!(request[12], 0xa0);
    assert!(request[13..].iter().all(|&byte| byte == 0));
}

#[test]
fn bigbigwon_uuid_parses_the_captured_identity_reply() {
    // Captured live from a linked GALE controller.
    let reply = [
        0xa5, 0x0c, 0xef, 0x42, 0x3b, 0xca, 0x92, 0x86, 0x3c, 0xba, 0x8d, 0x82,
    ];
    assert_eq!(
        bigbigwon_uuid(&reply),
        Some([0x42, 0x3b, 0xca, 0x92, 0x86, 0x3c, 0xba, 0x8d])
    );
    // The trailing zeros of the hidapi report must not disturb parsing.
    let mut padded = [0u8; 64];
    padded[..reply.len()].copy_from_slice(&reply);
    assert_eq!(
        bigbigwon_uuid(&padded),
        Some([0x42, 0x3b, 0xca, 0x92, 0x86, 0x3c, 0xba, 0x8d])
    );
    let mut corrupt = reply;
    corrupt[11] ^= 1;
    assert_eq!(bigbigwon_uuid(&corrupt), None);
    // Wrong opcode echo and truncated frames.
    assert_eq!(bigbigwon_uuid(&[0xa5, 0x0c, 0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0]), None);
    assert_eq!(bigbigwon_uuid(&[0xa5, 0x0c, 0xef, 0x42]), None);
    assert_eq!(bigbigwon_uuid(&[]), None);
}

#[test]
fn uid_anchors_identity_across_personalities_and_silent_docking() {
    let first = model::merge_readings(
        &[],
        vec![gamepad_reading("port-1", "423bca92863cba8d", 0x2104, Some(91))],
        1,
    );
    assert!(first[0].saved.id.starts_with("uid:"));
    assert_eq!(first[0].saved.uid, "423bca92863cba8d");
    let mut saved: Vec<_> = first.into_iter().map(|d| d.saved).collect();
    saved[0].alias = "客厅手柄".into();
    // Docked on the same port: idle personality pid, silent, no uid.
    let second = model::merge_readings(
        &saved,
        vec![gamepad_reading("port-1", "", 0x2106, None)],
        2,
    );
    assert_eq!(second.len(), 1);
    assert!(second[0].present);
    assert_eq!(second[0].saved.pid, 0x2106);
    assert_eq!(second[0].saved.uid, "423bca92863cba8d");
    assert!(second[0].saved.id.starts_with("uid:"));
    let saved: Vec<_> = second.into_iter().map(|d| d.saved).collect();
    let third = model::merge_readings(
        &saved,
        vec![gamepad_reading("port-1", "423bca92863cba8d", 0x2104, Some(88))],
        3,
    );
    assert_eq!(third.len(), 1);
    assert_eq!(third[0].saved.alias, "客厅手柄");
    assert_eq!(third[0].saved.battery, Some(88));
    assert_eq!(third[0].saved.pid, 0x2104);
}

#[test]
fn a_uid_reading_adopts_the_earlier_docked_record() {
    let first = model::merge_readings(&[], vec![gamepad_reading("port-1", "", 0x2106, None)], 1);
    assert!(first[0].saved.id.starts_with("local:"));
    let mut saved: Vec<_> = first.into_iter().map(|d| d.saved).collect();
    saved[0].alias = "客厅手柄".into();
    let second = model::merge_readings(
        &saved,
        vec![gamepad_reading("port-1", "423bca92863cba8d", 0x2104, Some(90))],
        2,
    );
    assert_eq!(second.len(), 1);
    assert!(second[0].saved.id.starts_with("uid:"));
    assert_eq!(second[0].saved.alias, "客厅手柄");
    assert_eq!(second[0].saved.uid, "423bca92863cba8d");
}

#[test]
fn different_uids_stay_separate_even_on_matching_reports() {
    let devices = model::merge_readings(
        &[],
        vec![
            gamepad_reading("port-1", "423bca92863cba8d", 0x2104, Some(91)),
            gamepad_reading("port-2", "aabbccddeeff0011", 0x2104, Some(55)),
        ],
        1,
    );
    assert_eq!(devices.len(), 2);
    let ids: Vec<_> = devices.iter().map(|d| d.saved.id.clone()).collect();
    assert!(ids[0] != ids[1]);
}
