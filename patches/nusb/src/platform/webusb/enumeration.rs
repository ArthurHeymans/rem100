use std::sync::Arc;

use wasm_bindgen_futures::{js_sys::Array, wasm_bindgen::JsCast, JsFuture};
use web_sys::UsbDevice;

use crate::{
    descriptors::ConfigurationDescriptor,
    maybe_future::{future::ActualFuture, Ready},
    platform::webusb::device::{extract_decriptors, extract_string},
    BusInfo, DeviceInfo, Error, ErrorKind, InterfaceInfo, MaybeFuture,
};

use super::UniqueUsbDevice;

pub fn list_devices() -> impl MaybeFuture<Output = Result<impl Iterator<Item = DeviceInfo>, Error>>
{
    async fn inner() -> Result<Vec<DeviceInfo>, Error> {
        let usb = super::usb()?;
        let devices = JsFuture::from(usb.get_devices()).await.map_err(|e| {
            log::error!("WebUSB devices could not be listed: {e:?}");
            Error::new(ErrorKind::Other, "WebUSB devices could not be listed")
        })?;

        let devices: Array = JsCast::unchecked_from_js(devices);

        let mut result = vec![];
        for device in devices {
            let device: UsbDevice = JsCast::unchecked_from_js(device);
            JsFuture::from(device.open()).await.map_err(|e| {
                log::error!("WebUSB device could not be opened: {e:?}");
                Error::new(ErrorKind::Other, "WebUSB device could not be opened")
            })?;

            let device = Arc::new(UniqueUsbDevice::new(device));

            let device_info = device_to_info(device.clone()).await?;
            result.push(device_info);
            JsFuture::from(device.close()).await.map_err(|e| {
                log::error!("WebUSB device could not be closed: {e:?}");
                Error::new(ErrorKind::Other, "WebUSB device could not be closed")
            })?;
        }

        Ok(result)
    }

    ActualFuture::new(async move { Ok(inner().await?.into_iter()) })
}

pub fn list_buses() -> impl MaybeFuture<Output = Result<impl Iterator<Item = BusInfo>, Error>> {
    Ready(Ok(vec![].into_iter()))
}

/// Create a DeviceInfo from a UsbDevice that was obtained via requestDevice().
///
/// This is useful when you need to use a device that was already granted permission
/// through the WebUSB requestDevice() flow, without having to re-list and re-open devices.
///
/// The device will be opened if not already open, and descriptors will be read.
/// The device is left open after this call.
pub fn device_info_from_webusb(
    device: UsbDevice,
) -> impl MaybeFuture<Output = Result<DeviceInfo, Error>> {
    ActualFuture::new(async move {
        log::info!(
            "device_info_from_webusb: device.opened() = {}",
            device.opened()
        );
        log::info!(
            "device_info_from_webusb: vendor_id = 0x{:04x}, product_id = 0x{:04x}",
            device.vendor_id(),
            device.product_id()
        );

        // Open the device if it's not already open
        if !device.opened() {
            log::info!("device_info_from_webusb: opening device...");
            JsFuture::from(device.open()).await.map_err(|e| {
                let err_str = format!("{:?}", e);
                log::error!("WebUSB device could not be opened: {}", err_str);
                // Also log to browser console
                web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&format!(
                    "WebUSB device.open() failed: {}",
                    err_str
                )));
                Error::new(ErrorKind::Other, "WebUSB device could not be opened")
            })?;
            log::info!("device_info_from_webusb: device opened successfully");
        } else {
            log::info!("device_info_from_webusb: device was already open");
        }

        let device = Arc::new(UniqueUsbDevice::new(device));
        log::info!("device_info_from_webusb: extracting device info...");
        let device_info = device_to_info(device.clone()).await?;
        log::info!("device_info_from_webusb: device info extracted");

        // Close the device so it can be reopened via DeviceInfo::open()
        log::info!("device_info_from_webusb: closing device...");
        JsFuture::from(device.close()).await.map_err(|e| {
            log::error!("WebUSB device could not be closed: {e:?}");
            Error::new(ErrorKind::Other, "WebUSB device could not be closed")
        })?;
        log::info!("device_info_from_webusb: device closed");

        Ok(device_info)
    })
}

pub(crate) async fn device_to_info(device: Arc<UniqueUsbDevice>) -> Result<DeviceInfo, Error> {
    Ok(DeviceInfo {
        vendor_id: device.vendor_id(),
        product_id: device.product_id(),
        usb_version: ((device.usb_version_major() as u16) << 8) | device.usb_version_minor() as u16,
        class: device.device_class(),
        subclass: device.device_subclass(),
        protocol: device.device_protocol(),
        speed: None,
        manufacturer_string: device.manufacturer_name(),
        product_string: device.product_name(),
        serial_number: device.serial_number(),
        interfaces: {
            let descriptors = extract_decriptors(&device).await?;
            let mut interfaces = vec![];
            for descriptor in descriptors.into_iter() {
                // TODO(webusb): Remove unwrap()
                let configuration = ConfigurationDescriptor::new(&descriptor).unwrap();
                for interface_group in configuration.interfaces() {
                    let alternate = interface_group.first_alt_setting();
                    let interface_string = if let Some(id) = alternate.string_index() {
                        Some(extract_string(&device, id.get() as u16).await?)
                    } else {
                        None
                    };

                    interfaces.push(InterfaceInfo {
                        interface_number: interface_group.interface_number(),
                        class: alternate.class(),
                        subclass: alternate.subclass(),
                        protocol: alternate.protocol(),
                        interface_string,
                    });
                }
            }
            interfaces
        },
        device: device.clone(),
    })
}
