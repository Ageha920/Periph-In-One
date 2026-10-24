use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedDevice {
    pub id: String,
    pub kind: String,
    pub hardware_name: String,
    pub serial: String,
    pub vid: u16,
    pub pid: u16,
    pub alias: String,
    pub battery: Option<u8>,
    pub last_success: Option<u64>,
    pub identity_conflict: bool,
}

#[derive(Clone, Debug)]
pub struct Reading {
    pub physical_id: String,
    pub kind: String,
    pub hardware_name: String,
    pub serial: String,
    pub vid: u16,
    pub pid: u16,
    pub battery: Option<u8>,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct Device {
    pub saved: SavedDevice,
    pub present: bool,
    pub fresh: bool,
    pub message: String,
}

/// Scope weak identifiers to one continuous observation in one application run.
/// A Windows ContainerId without a serial can be recycled for another receiver.
pub struct SessionTracker {
    session: String,
    next: u64,
    active: HashMap<(u16, u16, String), u64>,
}

impl SessionTracker {
    pub fn new(session: String) -> Self {
        Self {
            session,
            next: 0,
            active: HashMap::new(),
        }
    }

    pub fn track(&mut self, mut readings: Vec<Reading>) -> Vec<Reading> {
        let current: HashSet<_> = readings
            .iter()
            .map(|r| (r.vid, r.pid, r.physical_id.clone()))
            .collect();
        self.active.retain(|key, _| current.contains(key));
        for reading in &mut readings {
            let key = (reading.vid, reading.pid, reading.physical_id.clone());
            let epoch = *self.active.entry(key).or_insert_with(|| {
                self.next += 1;
                self.next
            });
            reading.physical_id = format!("{}:{epoch}:{}", self.session, reading.physical_id);
        }
        readings
    }
}

pub fn serial_key(vid: u16, pid: u16, serial: &str) -> String {
    // JSON tuple avoids ambiguous concatenations, and does not normalize case.
    format!("sn:{}", serde_json::to_string(&(vid, pid, serial)).unwrap())
}

fn name_used_as_serial(serial: &str, name: &str) -> bool {
    !serial.is_empty() && serial.trim().eq_ignore_ascii_case(name.trim())
}

pub fn merge_readings(saved: &[SavedDevice], readings: Vec<Reading>, now: u64) -> Vec<Device> {
    let mut devices: Vec<Device> = saved
        .iter()
        .cloned()
        .map(|saved| Device {
            saved,
            present: false,
            fresh: false,
            message: "设备已离线".into(),
        })
        .collect();
    let mut counts = HashMap::<String, usize>::new();
    let mut physical = HashSet::new();
    let readings: Vec<_> = readings
        .into_iter()
        .filter(|r| physical.insert((r.vid, r.pid, r.physical_id.clone())))
        .collect();
    for reading in &readings {
        if !reading.serial.is_empty() {
            *counts
                .entry(serial_key(reading.vid, reading.pid, &reading.serial))
                .or_default() += 1;
        }
    }
    // A serial collision is remembered: a later scan with only one receiver
    // must not silently attach a historical remark to the wrong receiver.
    for device in &mut devices {
        if name_used_as_serial(&device.saved.serial, &device.saved.hardware_name)
            || counts
                .get(&serial_key(
                    device.saved.vid,
                    device.saved.pid,
                    &device.saved.serial,
                ))
                .is_some_and(|n| *n > 1)
        {
            device.saved.identity_conflict = true;
        }
    }
    for reading in readings {
        let serial_id = serial_key(reading.vid, reading.pid, &reading.serial);
        let conflict = name_used_as_serial(&reading.serial, &reading.hardware_name)
            || counts.get(&serial_id).is_some_and(|n| *n > 1)
            || devices
                .iter()
                .any(|d| d.saved.id == serial_id && d.saved.identity_conflict);
        let id = if reading.serial.is_empty() || conflict {
            format!(
                "local:{}",
                serde_json::to_string(&(reading.vid, reading.pid, &reading.physical_id)).unwrap()
            )
        } else {
            serial_id
        };
        let index = devices
            .iter()
            .position(|d| d.saved.id == id)
            .unwrap_or_else(|| {
                devices.push(Device {
                    saved: SavedDevice {
                        id: id.clone(),
                        ..Default::default()
                    },
                    present: false,
                    fresh: false,
                    message: String::new(),
                });
                devices.len() - 1
            });
        let device = &mut devices[index];
        device.saved.kind = reading.kind;
        if !reading.hardware_name.is_empty() {
            device.saved.hardware_name = reading.hardware_name;
        }
        device.saved.serial = reading.serial;
        device.saved.vid = reading.vid;
        device.saved.pid = reading.pid;
        device.saved.identity_conflict |= conflict;
        device.present = true;
        device.fresh = reading.battery.is_some_and(|level| level <= 100);
        device.message = reading.message;
        if let Some(battery) = reading.battery.filter(|b| *b <= 100) {
            device.saved.battery = Some(battery);
            device.saved.last_success = Some(now);
        }
    }
    devices
}

/// Explicitly move an orphaned remark to a receiver selected by the user.
pub fn rebind(devices: &mut [Device], from: &str, to: &str) -> Result<(), &'static str> {
    if from == to {
        return Err("请选择不同的设备记录");
    }
    let source = devices
        .iter()
        .position(|d| d.saved.id == from && !d.present)
        .ok_or("原记录必须是离线设备")?;
    let target = devices
        .iter()
        .position(|d| d.saved.id == to && d.present)
        .ok_or("目标设备必须在线")?;
    if devices[source].saved.alias.is_empty() {
        return Err("原记录没有备注");
    }
    if !devices[target].saved.alias.is_empty() {
        return Err("请先清空目标设备的备注");
    }
    devices[target].saved.alias = std::mem::take(&mut devices[source].saved.alias);
    Ok(())
}
