use crate::ThermalMsgs;
use core::cell::RefCell;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;
use embedded_fans_async::{Fan, RpmSense};
use embedded_services::{error, info};

#[derive(Clone, Copy, Debug)]
pub enum FanError {
    Bus,
}

pub struct FanDevice<F: Fan + RpmSense> {
    device: RefCell<F>,
    pub(crate) rx: Channel<NoopRawMutex, ThermalMsgs, 1>,
    pub(crate) tx: Channel<NoopRawMutex, Result<ThermalMsgs, FanError>, 1>,
}

impl<F: Fan + RpmSense> FanDevice<F> {
    pub fn new(fan: F) -> Self {
        Self {
            device: RefCell::new(fan),
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
            ThermalMsgs::Oem(_) => error!("Unexpected message sent to fan"),
        }
    }
}
