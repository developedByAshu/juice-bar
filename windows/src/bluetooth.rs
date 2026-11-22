//! Batteries Windows reads itself from paired Bluetooth mice and keyboards, and the names it
//! remembers for them, which can name the same device when it is on its 2.4 GHz dongle.
//!
//! Everything comes from the device list Windows keeps (SetupAPI), paired devices that are off
//! included: the Bluetooth device node carries the name, the battery Windows last read and whether
//! it is connected; its HID collections, found through the shared container id, tell mouse from
//! keyboard.

use juicebar_shared::catalog::find_by_ids;
use juicebar_shared::device::{Battery, Connection, Device, DeviceKind, NameSource};
use std::collections::BTreeMap;
use windows_sys::core::GUID;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW, SetupDiGetDeviceInstanceIdW, SetupDiGetDevicePropertyW,
    DIGCF_ALLCLASSES, HDEVINFO, SP_DEVINFO_DATA,
};
use windows_sys::Win32::Devices::Properties::{DEVPKEY_Device_ClassGuid, DEVPKEY_Device_ContainerId, DEVPKEY_Device_FriendlyName, DEVPKEY_NAME};
use windows_sys::Win32::Foundation::{DEVPROPKEY, INVALID_HANDLE_VALUE};

/// The battery % Windows last read over Bluetooth (a byte), as shown in Settings.
const DEVPKEY_BLUETOOTH_BATTERY: DEVPROPKEY = DEVPROPKEY { fmtid: GUID::from_u128(0x104EA319_6EE2_4701_BD47_8DDBF425BBE5), pid: 2 };
/// True while the device is connected right now.
const DEVPKEY_DEVICE_IS_CONNECTED: DEVPROPKEY = DEVPROPKEY { fmtid: GUID::from_u128(0x83DA6326_97A6_4088_9453_A1923F573B29), pid: 15 };
/// The classic Bluetooth class of device; LE devices have none.
const DEVPKEY_BLUETOOTH_CLASS_OF_DEVICE: DEVPROPKEY = DEVPROPKEY { fmtid: GUID::from_u128(0x2BD67D8B_8BEB_48D5_87E0_6CDA3428040A), pid: 10 };
const GUID_DEVCLASS_MOUSE: u128 = 0x4D36E96F_E325_11CE_BFC1_08002BE10318;
const GUID_DEVCLASS_KEYBOARD: u128 = 0x4D36E96B_E325_11CE_BFC1_08002BE10318;
const PERIPHERAL_MAJOR_CLASS: u32 = 0x05;
const MAX_ID_UNITS: usize = 512;

/// One paired Bluetooth device, as Windows remembers it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PairedDevice {
    /// The 12 hex digit Bluetooth address, upper case.
    pub address: String,
    pub name: String,
    pub battery: Option<u8>,
    pub is_connected: bool,
    pub class_of_device: Option<u32>,
    /// The container id shared by the device and its HID collections, as one number.
    pub container: Option<u128>,
}

/// What a device's HID collections say about it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Collections {
    pub mice: u32,
    pub keyboards: u32,
    pub vendor_id: u16,
    pub product_id: u16,
}

/// Returns the address in a Bluetooth instance id such as `BTHLE\DEV_C99C176C4EBB\...`.
pub fn bluetooth_address(instance_id: &str) -> Option<String> {
    let upper = instance_id.to_uppercase();
    let rest = upper.strip_prefix("BTHLE\\DEV_").or_else(|| upper.strip_prefix("BTHENUM\\DEV_"))?;
    let address: String = rest.chars().take_while(char::is_ascii_hexdigit).collect();
    (address.len() == 12).then_some(address)
}

/// Returns the vendor and product ids in a HID instance id such as `HID\{...}_DEV_VID&0204F3_PID&026D_...`.
pub fn hid_ids(instance_id: &str) -> Option<(u16, u16)> {
    let upper = instance_id.to_uppercase();
    let hex_after = |marker: &str, digits: usize| {
        let start = upper.find(marker)? + marker.len();
        let text: String = upper[start..].chars().take_while(char::is_ascii_hexdigit).collect();
        // Bluetooth vendor ids carry a 2 digit source prefix (02 = USB-IF) before the 4 digit id.
        let text = if text.len() > digits { &text[text.len() - digits..] } else { &text };
        u16::from_str_radix(text, 16).ok()
    };
    Some((hex_after("VID&", 4)?, hex_after("PID&", 4)?))
}

/**
 * Tells a mouse from a keyboard. A known id decides; then the classic class of device; then the
 * collections: Bluetooth mice also expose one keyboard collection for their buttons, and keyboards
 * one mouse collection for media keys, but a keyboard has two keyboard collections.
 */
pub fn kind_of(paired: &PairedDevice, collections: &Collections) -> DeviceKind {
    if let Some(known) = find_by_ids(collections.vendor_id, collections.product_id) {
        return known.kind;
    }
    if let Some(class) = paired.class_of_device {
        if (class >> 8) & 0x1F == PERIPHERAL_MAJOR_CLASS {
            match (class >> 6) & 0x03 {
                0b01 => return DeviceKind::Keyboard,
                0b10 => return DeviceKind::Mouse,
                _ => {}
            }
        }
    }
    match (collections.mice, collections.keyboards) {
        (_, 2..) => DeviceKind::Keyboard,
        (1.., _) => DeviceKind::Mouse,
        (0, 1) => DeviceKind::Keyboard,
        _ => DeviceKind::Other,
    }
}

/// Builds the menu row for a connected paired device. The OS-read battery counts as device reported.
pub fn paired_to_device(paired: &PairedDevice, kind: DeviceKind, collections: &Collections) -> Device {
    Device {
        id: format!("bt:{}", paired.address.to_lowercase()),
        kind,
        connection: Connection::Bluetooth,
        name: paired.name.clone(),
        name_source: NameSource::DeviceReported,
        battery: paired.battery.map(|percent| Battery { percent: percent.min(100), is_charging: false }),
        protocol: Some("bluetooth"),
        vendor_id: collections.vendor_id,
        product_id: collections.product_id,
    }
}

/// Reads one property's raw bytes from a device in a list.
fn read_property(set: HDEVINFO, info: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<Vec<u8>> {
    let mut kind = 0u32;
    let mut buffer = vec![0u8; 512];
    let mut required = 0u32;
    // SAFETY: the list and entry are valid, and the buffer size passed matches the buffer.
    let found = unsafe { SetupDiGetDevicePropertyW(set, info, key, &mut kind, buffer.as_mut_ptr(), buffer.len() as u32, &mut required, 0) };
    (found != 0).then(|| {
        buffer.truncate(required as usize);
        buffer
    })
}

fn read_text(set: HDEVINFO, info: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<String> {
    let bytes = read_property(set, info, key)?;
    let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes(*pair)).take_while(|unit| *unit != 0).collect();
    Some(String::from_utf16_lossy(&units)).filter(|text| !text.trim().is_empty())
}

/// Reads a GUID property as one number, laid out as `GUID::from_u128` takes it.
fn read_guid(set: HDEVINFO, info: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<u128> {
    let bytes = read_property(set, info, key)?;
    (bytes.len() >= 16).then(|| {
        let data1 = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let data2 = u16::from_le_bytes([bytes[4], bytes[5]]);
        let data3 = u16::from_le_bytes([bytes[6], bytes[7]]);
        let mut data4 = [0u8; 8];
        data4.copy_from_slice(&bytes[8..16]);
        (u128::from(data1) << 96) | (u128::from(data2) << 80) | (u128::from(data3) << 64) | u128::from(u64::from_be_bytes(data4))
    })
}

/// Reads every paired Bluetooth device and the HID collections of each, from Windows' device list.
fn read_paired() -> Vec<(PairedDevice, Collections)> {
    let mut paired: BTreeMap<String, PairedDevice> = BTreeMap::new();
    let mut collections: Vec<(u128, u128, u16, u16)> = Vec::new();
    // SAFETY: the list is created here, read within its bounds, and destroyed once at the end.
    unsafe {
        // Every device Windows knows, present or not, so a paired device that is off still counts.
        let set = SetupDiGetClassDevsW(std::ptr::null(), std::ptr::null(), std::ptr::null_mut(), DIGCF_ALLCLASSES);
        if set == INVALID_HANDLE_VALUE as HDEVINFO {
            return Vec::new();
        }
        let mut index = 0;
        loop {
            let mut info: SP_DEVINFO_DATA = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
            if SetupDiEnumDeviceInfo(set, index, &mut info) == 0 {
                break;
            }
            index += 1;
            let mut buffer = [0u16; MAX_ID_UNITS];
            let mut length = 0u32;
            if SetupDiGetDeviceInstanceIdW(set, &info, buffer.as_mut_ptr(), buffer.len() as u32, &mut length) == 0 {
                continue;
            }
            let instance_id = String::from_utf16_lossy(&buffer[..(length as usize).saturating_sub(1)]);
            if let Some(address) = bluetooth_address(&instance_id) {
                let name = read_text(set, &info, &DEVPKEY_Device_FriendlyName).or_else(|| read_text(set, &info, &DEVPKEY_NAME)).unwrap_or_default();
                let battery = read_property(set, &info, &DEVPKEY_BLUETOOTH_BATTERY).and_then(|bytes| bytes.first().copied());
                let is_connected = read_property(set, &info, &DEVPKEY_DEVICE_IS_CONNECTED).is_some_and(|bytes| bytes.first().is_some_and(|byte| *byte != 0));
                let class_of_device = read_property(set, &info, &DEVPKEY_BLUETOOTH_CLASS_OF_DEVICE).filter(|bytes| bytes.len() >= 4).map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
                let container = read_guid(set, &info, &DEVPKEY_Device_ContainerId);
                // A dual-mode device has a classic and an LE node; merge them, keeping what each knows.
                let entry = paired.entry(address.clone()).or_insert_with(|| PairedDevice { address, ..PairedDevice::default() });
                if entry.name.is_empty() {
                    entry.name = name.trim().to_string();
                }
                entry.battery = entry.battery.or(battery);
                entry.is_connected |= is_connected;
                entry.class_of_device = entry.class_of_device.or(class_of_device);
                entry.container = entry.container.or(container);
            } else if instance_id.to_uppercase().starts_with("HID\\") {
                let (Some(class), Some(container)) = (read_guid(set, &info, &DEVPKEY_Device_ClassGuid), read_guid(set, &info, &DEVPKEY_Device_ContainerId)) else {
                    continue;
                };
                let (vendor_id, product_id) = hid_ids(&instance_id).unwrap_or_default();
                collections.push((container, class, vendor_id, product_id));
            }
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    paired
        .into_values()
        .filter(|device| !device.name.is_empty())
        .map(|device| {
            let mut summary = Collections::default();
            if let Some(container) = device.container {
                for (_, class, vendor_id, product_id) in collections.iter().filter(|(owner, ..)| *owner == container) {
                    summary.mice += u32::from(*class == GUID_DEVCLASS_MOUSE);
                    summary.keyboards += u32::from(*class == GUID_DEVCLASS_KEYBOARD);
                    if summary.vendor_id == 0 {
                        (summary.vendor_id, summary.product_id) = (*vendor_id, *product_id);
                    }
                }
            }
            (device, summary)
        })
        .collect()
}

/**
 * Returns the paired Bluetooth mice and keyboards that are connected now, with the battery Windows
 * read. One that is off is left out: it is usually the same device, now on its dongle.
 */
pub fn bluetooth_devices() -> Vec<Device> {
    read_paired()
        .into_iter()
        .filter(|(device, _)| device.is_connected)
        .filter_map(|(device, collections)| {
            let kind = kind_of(&device, &collections);
            (kind != DeviceKind::Other).then(|| paired_to_device(&device, kind, &collections))
        })
        .collect()
}

/// Returns the names Windows remembers for paired Bluetooth devices of a kind, connected or not.
pub fn remembered_bluetooth_names(kind: DeviceKind) -> Vec<String> {
    remembered_names_by_kind().into_iter().filter(|(of_kind, _)| *of_kind == kind).map(|(_, name)| name).collect()
}

/// Returns every remembered name with its kind, from one pass over the device list, for a whole read.
pub fn remembered_names_by_kind() -> Vec<(DeviceKind, String)> {
    read_paired().into_iter().map(|(device, collections)| (kind_of(&device, &collections), device.name)).collect()
}
