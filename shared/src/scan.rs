//! Finds the dongles plugged in and reads each one, the same on Mac and Windows.

use std::collections::BTreeMap;

use crate::catalog::{find_by_ids, find_known};
use crate::device::{Connection, Device, DeviceKind, NameSource};
use crate::names::{resolve_name, NameCandidates};
use crate::protocols::{protocol_by_id, Reading};
use crate::report::InterfaceSummary;
use crate::settings::Settings;
use crate::transport::HidapiTransport;

/// Generic Desktop usages that tell a mouse from a keyboard.
const USAGE_PAGE_GENERIC_DESKTOP: u16 = 0x01;
const USAGE_MOUSE: u16 = 0x02;
const USAGE_KEYBOARD: u16 = 0x06;

/// One physical USB device with all of its HID interfaces.
#[derive(Clone, Debug)]
pub struct UsbDevice {
    pub vendor_id: u16,
    pub product_id: u16,
    pub serial: String,
    pub product: String,
    pub interfaces: Vec<(InterfaceSummary, std::ffi::CString)>,
}

impl UsbDevice {
    /// Returns the stable id used for renames and hiding. The serial is the device's own text, so it
    /// is cleaned as the settings file saves it, and a saved rename still matches after a restart.
    pub fn id(&self) -> String {
        format!("{:04x}:{:04x}:{}", self.vendor_id, self.product_id, crate::settings::clean_key(&self.serial))
    }

    /// Guesses mouse or keyboard from the interfaces, preferring the catalog.
    pub fn kind(&self) -> DeviceKind {
        if let Some(known) = find_by_ids(self.vendor_id, self.product_id) {
            return known.kind;
        }
        let has_usage = |usage| self.interfaces.iter().any(|(summary, _)| summary.usage_page == USAGE_PAGE_GENERIC_DESKTOP && summary.usage == usage);
        if has_usage(USAGE_MOUSE) {
            DeviceKind::Mouse
        } else if has_usage(USAGE_KEYBOARD) {
            DeviceKind::Keyboard
        } else {
            DeviceKind::Other
        }
    }
}

/// Lists the USB HID devices that look like a mouse or keyboard, grouped per physical device.
pub fn list_usb_devices(api: &hidapi::HidApi) -> Vec<UsbDevice> {
    let mut grouped: BTreeMap<(u16, u16, String), UsbDevice> = BTreeMap::new();
    for info in api.device_list().filter(|info| matches!(info.bus_type(), hidapi::BusType::Usb)) {
        let serial = info.serial_number().unwrap_or_default().to_string();
        let key = (info.vendor_id(), info.product_id(), serial.clone());
        let entry = grouped.entry(key).or_insert_with(|| UsbDevice {
            vendor_id: info.vendor_id(),
            product_id: info.product_id(),
            serial,
            product: info.product_string().unwrap_or_default().to_string(),
            interfaces: Vec::new(),
        });
        entry.interfaces.push((InterfaceSummary { usage_page: info.usage_page(), usage: info.usage() }, info.path().to_owned()));
    }
    grouped.into_values().filter(|device| device.kind() != DeviceKind::Other || find_by_ids(device.vendor_id, device.product_id).is_some()).collect()
}

/// Reads one USB device with its catalog protocol. Returns the reading and the protocol id.
pub fn read_usb_device(api: &hidapi::HidApi, device: &UsbDevice) -> (Reading, Option<&'static str>) {
    for (summary, path) in &device.interfaces {
        let Some(known) = find_known(device.vendor_id, device.product_id, summary.usage_page) else {
            continue;
        };
        let Some(protocol) = protocol_by_id(known.protocol) else {
            continue;
        };
        let Ok(opened) = api.open_path(path) else {
            continue;
        };
        let mut transport = HidapiTransport { device: opened };
        if let Ok(reading) = protocol.read_battery(&mut transport) {
            if reading != Reading::NoAnswer {
                return (reading, Some(known.protocol));
            }
        }
    }
    // A known device that did not answer is asleep or off, not unknown.
    let known = device.interfaces.iter().find_map(|(summary, _)| find_known(device.vendor_id, device.product_id, summary.usage_page));
    known.map_or((Reading::NoAnswer, None), |known| (Reading::DeviceOffline, Some(known.protocol)))
}

/// Builds the menu row for a USB device from its reading, the settings and remembered Bluetooth names.
pub fn build_device(device: &UsbDevice, reading: &Reading, protocol: Option<&'static str>, settings: &Settings, bluetooth_names: &[String]) -> Device {
    let id = device.id();
    let kind = device.kind();
    let known = find_by_ids(device.vendor_id, device.product_id);
    let confirmed = settings.confirmed_bluetooth_names.get(&id).cloned();
    let candidates = NameCandidates {
        user_rename: settings.renames.get(&id).cloned(),
        device_reported: None,
        bluetooth_memory: confirmed.map_or_else(|| bluetooth_names.to_vec(), |name| vec![name]),
        catalog: known.map(|known| (known.family_name.to_string(), known.brand_shared)),
        usb_product: Some(device.product.clone()),
    };
    let (name, name_source) = resolve_name(kind, &candidates);
    let is_wireless = known.is_none_or(|known| known.is_wireless);
    Device {
        id,
        kind,
        connection: if is_wireless { Connection::Dongle } else { Connection::Usb },
        name,
        name_source: if protocol.is_none() && name_source != NameSource::UserRename { NameSource::Unknown } else { name_source },
        battery: match reading {
            Reading::Battery(battery) => Some(*battery),
            _ => None,
        },
        protocol,
        vendor_id: device.vendor_id,
        product_id: device.product_id,
    }
}

/// One dongle with its menu row, so a row can be probed or reported later.
#[derive(Clone, Debug)]
pub struct ScannedDevice {
    pub usb: UsbDevice,
    pub device: Device,
}

/// Reads every plugged-in dongle and builds its row, hidden ones included (Settings lists them to unhide;
/// the menu and icon filter them). `bluetooth_names` gives remembered names per kind.
pub fn scan_dongles(api: &hidapi::HidApi, settings: &Settings, bluetooth_names: &dyn Fn(DeviceKind) -> Vec<String>) -> Vec<ScannedDevice> {
    list_usb_devices(api)
        .into_iter()
        .map(|usb| {
            let (reading, protocol) = read_usb_device(api, &usb);
            let device = build_device(&usb, &reading, protocol, settings, &bluetooth_names(usb.kind()));
            ScannedDevice { usb, device }
        })
        .collect()
}

/// The one-time question before the first Try to detect, the same on Mac and Windows.
pub const PROBE_QUESTION: &str = "Send a battery read command to this device?";
pub const PROBE_DETAIL: &str = "Try to detect sends the battery read command of each keyboard and mouse Juice Bar knows to this device's vendor channel, and listens for an answer. It only asks and never changes a setting, but the device is not one Juice Bar knows. Juice Bar asks this once.";

/// Far above any real report descriptor; HID caps them at 4096 bytes.
const DESCRIPTOR_BUFFER_BYTES: usize = 4096;

/**
 * Tries every protocol's battery read on the device's vendor interfaces. Runs only when the user
 * clicks Try to detect and has agreed to it once. A protocol that sends a command sends it only to an
 * interface whose report descriptor declares that command's report id; when the descriptor cannot be
 * read, the usage page alone decides, as before.
 */
pub fn probe_usb_device(api: &hidapi::HidApi, device: &UsbDevice) -> (Reading, Option<&'static str>) {
    for protocol in crate::protocols::all_protocols() {
        for (_, path) in device.interfaces.iter().filter(|(summary, _)| summary.usage_page == protocol.usage_page()) {
            let Ok(opened) = api.open_path(path) else {
                continue;
            };
            if let Some(report_id) = protocol.command_report_id() {
                let mut descriptor = vec![0u8; DESCRIPTOR_BUFFER_BYTES];
                if let Ok(length) = opened.get_report_descriptor(&mut descriptor) {
                    if length > 0 && !crate::protocols::declares_report_id(&descriptor[..length.min(DESCRIPTOR_BUFFER_BYTES)], report_id) {
                        continue;
                    }
                }
            }
            let mut transport = HidapiTransport { device: opened };
            if let Ok(Reading::Battery(battery)) = protocol.read_battery(&mut transport) {
                return (Reading::Battery(battery), Some(protocol.id()));
            }
        }
    }
    (Reading::NoAnswer, None)
}
