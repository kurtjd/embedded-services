//! Fan Device
use super::DeviceId;
use crate::intrusive_list;
use core::cell::RefCell;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;
use embedded_fans_async as fan_traits;

/// Fan error type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// An unknown error occurred
    Unknown,
    // TODO: More errors
}

/// Fan request
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Current RPM
    CurRpm,
    /// Set RPM
    SetRpm(u32),
}

/// Fan response
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Response {
    /// Acknowledge request and return data
    Ack(u32),
}

/// Fan device struct
pub struct Device {
    /// Intrusive list node
    node: intrusive_list::Node,
    /// Device ID
    id: DeviceId,
    /// Channel for requests to the device
    request: Channel<NoopRawMutex, Request, 1>,
    /// Channel for responses from the device
    response: Channel<NoopRawMutex, Result<Response, Error>, 1>,
}

impl Device {
    /// Create a new sensor device
    pub fn new(id: DeviceId) -> Self {
        Self {
            node: intrusive_list::Node::uninit(),
            id,
            request: Channel::new(),
            response: Channel::new(),
        }
    }

    /// Get the device ID
    pub fn id(&self) -> DeviceId {
        self.id
    }

    /// Wait for a request
    pub async fn wait_request(&self) -> Request {
        self.request.receive().await
    }

    /// Send a response
    pub async fn send_response(&self, response: Result<Response, Error>) {
        self.response.send(response).await;
    }

    /// Execute request and wait for response
    pub async fn execute_request(&self, request: Request) -> Result<Response, Error> {
        self.request.send(request).await;
        self.response.receive().await
    }
}

impl intrusive_list::NodeContainer for Device {
    fn get_node(&self) -> &crate::Node {
        &self.node
    }
}

/// Fan struct containing device for comms and driver
pub struct Fan<T: fan_traits::Fan + fan_traits::RpmSense> {
    /// Underlying device
    pub device: Device,
    /// Underlying driver
    pub driver: RefCell<T>,
}

impl<T: fan_traits::Fan + fan_traits::RpmSense> Fan<T> {
    /// New fan
    pub fn new(id: DeviceId, driver: T) -> Self {
        Self {
            device: Device::new(id),
            driver: RefCell::new(driver),
        }
    }

    /// Process request for fan
    /// If this functionality is too generic, a custom request process method can be written
    pub async fn process_request(&self) {
        let request = self.device.wait_request().await;
        match request {
            Request::CurRpm => {
                let rpm = self.driver.borrow_mut().rpm().await.unwrap();
                self.device.send_response(Ok(Response::Ack(rpm as u32))).await;
            }
            Request::SetRpm(rpm) => {
                self.driver.borrow_mut().set_speed_rpm(rpm as u16).await.unwrap();
                self.device.send_response(Ok(Response::Ack(rpm))).await;
            }
        }
    }
}
