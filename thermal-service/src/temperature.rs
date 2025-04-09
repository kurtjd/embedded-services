use crate::ThermalMsgs;
use core::cell::RefCell;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;
use embedded_sensors_hal_async::temperature::{TemperatureSensor, TemperatureThresholdWait};
use embedded_services::{error, info};

#[derive(Clone, Copy, Debug)]
pub enum TemperatureSensorError {
    Bus,
}

pub struct TemperatureSensorDevice<T: TemperatureSensor + TemperatureThresholdWait> {
    device: RefCell<T>,
    pub(crate) rx: Channel<NoopRawMutex, ThermalMsgs, 1>,
    pub(crate) tx: Channel<NoopRawMutex, Result<ThermalMsgs, TemperatureSensorError>, 1>,
}

impl<T: TemperatureSensor + TemperatureThresholdWait> TemperatureSensorDevice<T> {
    pub fn new(temperature_sensor: T) -> Self {
        Self {
            device: RefCell::new(temperature_sensor),
            rx: Channel::new(),
            tx: Channel::new(),
        }
    }

    pub async fn process_service_message(&self) {
        let rx_msg = self.rx.receive().await;

        match rx_msg {
            ThermalMsgs::Acpi(msg) => match msg {
                _ => todo!(),
            },
            ThermalMsgs::Oem(_) => error!("Unexpected message sent to temperature sensor"),
        }
    }
}
