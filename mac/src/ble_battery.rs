//! Battery levels read straight from connected Bluetooth mice and keyboards, through the standard
//! Battery Service (0x180F, level 0x2A19). macOS keeps a level only for Apple devices, so without
//! this most Bluetooth mice show no %. Everything runs on the main thread: CoreBluetooth calls back
//! on the main queue, reads are asynchronous, and each answer rebuilds the menu.
//!
//! The manager (and so the Bluetooth permission prompt) is created only once macOS lists a connected
//! Bluetooth mouse or keyboard, so a Mac without one never sees the prompt.

use crate::bluetooth::BluetoothEntry;
use crate::delegate::main_thread;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread, MainThreadOnly};
use objc2_core_bluetooth::{CBCentralManager, CBCentralManagerDelegate, CBCharacteristic, CBCharacteristicProperties, CBManagerState, CBPeripheral, CBPeripheralDelegate, CBService, CBUUID};
use objc2_foundation::{NSArray, NSError, NSString};
use std::cell::RefCell;
use std::collections::HashMap;

const BATTERY_SERVICE: &str = "180F";
const BATTERY_LEVEL: &str = "2A19";

struct Reader {
    manager: Retained<CBCentralManager>,
    delegate: Retained<BleDelegate>,
    /// Peripherals we hold a connection to, by their identifier, so CoreBluetooth keeps them alive.
    peripherals: HashMap<String, Retained<CBPeripheral>>,
    /// The last level each device reported, by the name macOS shows for it.
    levels: HashMap<String, u8>,
}

thread_local! {
    static READER: RefCell<Option<Reader>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[name = "JuiceBarBleDelegate"]
    struct BleDelegate;

    unsafe impl NSObjectProtocol for BleDelegate {}

    unsafe impl CBCentralManagerDelegate for BleDelegate {
        #[unsafe(method(centralManagerDidUpdateState:))]
        fn did_update_state(&self, central: &CBCentralManager) {
            // SAFETY: plain property read.
            if unsafe { central.state() } == CBManagerState::PoweredOn {
                connect_all();
            }
        }

        #[unsafe(method(centralManager:didConnectPeripheral:))]
        fn did_connect(&self, _central: &CBCentralManager, peripheral: &CBPeripheral) {
            // SAFETY: the peripheral is connected; the services array holds CBUUIDs.
            unsafe { peripheral.discoverServices(Some(&NSArray::from_retained_slice(&[uuid(BATTERY_SERVICE)]))) };
        }

        #[unsafe(method(centralManager:didDisconnectPeripheral:error:))]
        fn did_disconnect(&self, _central: &CBCentralManager, peripheral: &CBPeripheral, _error: Option<&NSError>) {
            let name = peripheral_name(peripheral);
            let id = peripheral_id(peripheral);
            with_reader(|reader| {
                reader.peripherals.remove(&id);
                reader.levels.remove(&name);
            });
            crate::app::on_bluetooth_battery();
        }
    }

    unsafe impl CBPeripheralDelegate for BleDelegate {
        #[unsafe(method(peripheral:didDiscoverServices:))]
        fn did_discover_services(&self, peripheral: &CBPeripheral, _error: Option<&NSError>) {
            // SAFETY: services were just discovered on this connected peripheral.
            for service in unsafe { peripheral.services() }.unwrap_or_default().iter() {
                unsafe { peripheral.discoverCharacteristics_forService(Some(&NSArray::from_retained_slice(&[uuid(BATTERY_LEVEL)])), &service) };
            }
        }

        #[unsafe(method(peripheral:didDiscoverCharacteristicsForService:error:))]
        fn did_discover_characteristics(&self, peripheral: &CBPeripheral, service: &CBService, _error: Option<&NSError>) {
            // SAFETY: characteristics were just discovered for this service.
            for characteristic in unsafe { service.characteristics() }.unwrap_or_default().iter() {
                unsafe {
                    peripheral.readValueForCharacteristic(&characteristic);
                    // Devices that push their level update the menu without polling.
                    if characteristic.properties().contains(CBCharacteristicProperties::Notify) {
                        peripheral.setNotifyValue_forCharacteristic(true, &characteristic);
                    }
                }
            }
        }

        #[unsafe(method(peripheral:didUpdateValueForCharacteristic:error:))]
        fn did_update_value(&self, peripheral: &CBPeripheral, characteristic: &CBCharacteristic, error: Option<&NSError>) {
            if error.is_some() {
                return;
            }
            // SAFETY: plain property read; the level is one byte, 0 to 100.
            let Some(level) = unsafe { characteristic.value() }.and_then(|data| data.to_vec().first().copied()).filter(|level| *level <= 100) else {
                return;
            };
            let name = peripheral_name(peripheral);
            let changed = with_reader(|reader| reader.levels.insert(name, level) != Some(level)).unwrap_or(false);
            if changed {
                crate::app::on_bluetooth_battery();
            }
        }
    }
);

fn uuid(text: &str) -> Retained<CBUUID> {
    // SAFETY: a valid 16-bit UUID string.
    unsafe { CBUUID::UUIDWithString(&NSString::from_str(text)) }
}

fn peripheral_name(peripheral: &CBPeripheral) -> String {
    // SAFETY: plain property read.
    unsafe { peripheral.name() }.map(|name| name.to_string().trim().to_string()).unwrap_or_default()
}

fn peripheral_id(peripheral: &CBPeripheral) -> String {
    // SAFETY: plain property read.
    unsafe { peripheral.identifier() }.UUIDString().to_string()
}

fn with_reader<T>(work: impl FnOnce(&mut Reader) -> T) -> Option<T> {
    READER.with(|cell| cell.borrow_mut().as_mut().map(work))
}

/// Connects to every peripheral macOS has connected that offers the Battery Service. Each level is read
/// ONCE per connection, then only notifications update it: some mice (RivieraWaves firmware, e.g. the
/// XS Flow) raise the value on every read, so polling would make the % climb on its own.
fn connect_all() {
    let Some((manager, delegate)) = with_reader(|reader| (reader.manager.clone(), reader.delegate.clone())) else {
        return;
    };
    // SAFETY: the manager is powered on; the services array holds CBUUIDs.
    let connected = unsafe { manager.retrieveConnectedPeripheralsWithServices(&NSArray::from_retained_slice(&[uuid(BATTERY_SERVICE)])) };
    for peripheral in connected.iter() {
        let id = peripheral_id(&peripheral);
        let is_new = with_reader(|reader| !reader.peripherals.contains_key(&id)).unwrap_or(false);
        if is_new {
            // SAFETY: the delegate lives as long as the reader.
            unsafe {
                peripheral.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
                manager.connectPeripheral_options(&peripheral, None);
            }
            with_reader(|reader| reader.peripherals.insert(id, peripheral.clone()));
        }
    }
}

/// Starts the reader the first time a connected Bluetooth mouse or keyboard is seen, then refreshes it.
pub fn refresh(entries: &[BluetoothEntry]) {
    let has_input_device = entries.iter().any(|entry| entry.is_connected);
    let is_started = READER.with(|cell| cell.borrow().is_some());
    if !is_started {
        if !has_input_device {
            return;
        }
        let delegate: Retained<BleDelegate> = unsafe { msg_send![BleDelegate::alloc(main_thread()), init] };
        // SAFETY: a nil queue means the main queue, where every callback above expects to run.
        let manager = unsafe { CBCentralManager::initWithDelegate_queue(CBCentralManager::alloc(), Some(ProtocolObject::from_ref(&*delegate)), None) };
        READER.with(|cell| *cell.borrow_mut() = Some(Reader { manager, delegate, peripherals: HashMap::new(), levels: HashMap::new() }));
        return; // centralManagerDidUpdateState connects once Bluetooth is ready.
    }
    // SAFETY: plain property read.
    if with_reader(|reader| unsafe { reader.manager.state() } == CBManagerState::PoweredOn).unwrap_or(false) {
        connect_all();
    }
}

/// Fills a connected entry's missing battery from the level its device last reported.
pub fn fill_levels(entries: &mut [BluetoothEntry]) {
    with_reader(|reader| {
        for entry in entries.iter_mut().filter(|entry| entry.is_connected && entry.battery.is_none()) {
            entry.battery = reader.levels.get(&entry.name).copied();
        }
    });
}
