//! Fan Device
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::mutex::Mutex;
use embedded_fans_async as fan_traits;
use embedded_services::{intrusive_list, Node};

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

/// Device ID new type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct DeviceId(pub u8);

/// Fan device struct
pub struct Device {
    /// Intrusive list node
    node: Node,
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
            node: Node::uninit(),
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
    fn get_node(&self) -> &Node {
        &self.node
    }
}

/// Fan struct containing device for comms and driver
pub struct Wrapper<T: fan_traits::Fan + fan_traits::RpmSense> {
    /// Underlying device
    pub device: Device,
    /// Underlying driver
    pub driver: Mutex<NoopRawMutex, T>,
}

impl<T: fan_traits::Fan + fan_traits::RpmSense> Wrapper<T> {
    /// New fan
    pub fn new(id: DeviceId, driver: T) -> Self {
        Self {
            device: Device::new(id),
            driver: Mutex::new(driver),
        }
    }

    /// Process request for fan
    /// If this functionality is too generic, a custom request process method can be written
    pub async fn wait_and_process(&self) {
        let request = self.device.wait_request().await;
        self.process_request(request).await;
    }

    async fn process_request(&self, request: Request) {
        match request {
            Request::CurRpm => {
                let rpm = self.driver.lock().await.rpm().await.unwrap();
                self.device.send_response(Ok(Response::Ack(rpm as u32))).await;
            }
            Request::SetRpm(rpm) => {
                self.driver.lock().await.set_speed_rpm(rpm as u16).await.unwrap();
                self.device.send_response(Ok(Response::Ack(rpm))).await;
            }
        }
    }
}

/// Should be called by a wrapper task per fan (since tasks themselves cannot be generic)
pub async fn fan_task<T: fan_traits::Fan + fan_traits::RpmSense>(fan: &'static Wrapper<T>) {
    loop {
        fan.wait_and_process().await;
    }
}
