//! Builds the "Report device" link: a prefilled GitHub issue with ids only, nothing personal.

use crate::catalog::find_by_ids;
use crate::device::Device;

const ISSUES_URL: &str = "https://github.com/developedByAshu/juice-bar/issues/new";

/// The HID interfaces of the device, for the report body.
#[derive(Clone, Debug)]
pub struct InterfaceSummary {
    pub usage_page: u16,
    pub usage: u16,
}

/**
 * Returns the name the report gives the device: the catalog's family name, else the name the USB
 * device gives itself. Never the user's rename or a Bluetooth name, which can be personal.
 */
pub fn report_name(device: &Device, usb_product: Option<&str>) -> String {
    let product = usb_product.map(|product| product.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string()).filter(|product| !product.is_empty());
    find_by_ids(device.vendor_id, device.product_id).map(|known| known.family_name.to_string()).or(product).unwrap_or_else(|| "not given".to_string())
}

/// Returns the issue body the user sees before sending. `usb_product` is the USB product string.
pub fn report_body(device: &Device, usb_product: Option<&str>, interfaces: &[InterfaceSummary], os: &str, app_version: &str) -> String {
    let pages: Vec<String> = interfaces.iter().map(|interface| format!("0x{:04X}:0x{:04X}", interface.usage_page, interface.usage)).collect();
    format!(
        "Product name: {}\nVendor id: 0x{:04X}\nProduct id: 0x{:04X}\nConnection: {:?}\nProtocol matched: {}\nHID interfaces: {}\nOS: {os}\nJuice Bar: {app_version}\n\nReal name and model (please add):\n",
        report_name(device, usb_product),
        device.vendor_id,
        device.product_id,
        device.connection,
        device.protocol.unwrap_or("none"),
        pages.join(", "),
    )
}

/// Returns the GitHub new-issue URL with title and body filled in.
pub fn report_url(device: &Device, usb_product: Option<&str>, interfaces: &[InterfaceSummary], os: &str, app_version: &str) -> String {
    let title = format!("Device report: {:04x}:{:04x}", device.vendor_id, device.product_id);
    format!("{ISSUES_URL}?title={}&body={}", encode(&title), encode(&report_body(device, usb_product, interfaces, os, app_version)))
}

/// Percent-encodes text for a URL query value.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (byte as char).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}
