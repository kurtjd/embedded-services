//! Sensor Device
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::mutex::Mutex;
use embedded_sensors_hal_async::temperature::TemperatureSensor;
use embedded_services::{intrusive_list, Node};

/// Sensor error type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// An unknown error occurred
    Unknown,
    // TODO: More errors
}

/// Sensor request
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Current temperature measurement
    CurTemp,
}

/// Sensor response
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Response {
    /// Acknowledge request and return data
    Ack(f32),
}

/// Device ID new type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct DeviceId(pub u8);

/// Sensor device struct
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

/// Sensor struct containing device for comms and driver
pub struct Wrapper<T: TemperatureSensor> {
    /// Underlying device
    pub device: Device,
    /// Underlying driver
    pub driver: Mutex<NoopRawMutex, T>,
}

impl<T: TemperatureSensor> Wrapper<T> {
    /// New sensor
    pub fn new(id: DeviceId, driver: T) -> Self {
        Self {
            device: Device::new(id),
            driver: Mutex::new(driver),
        }
    }

    /// Process request for sensor
    /// If this functionality is too generic, a custom request process method can be written
    pub async fn wait_and_process(&self) {
        let request = self.device.wait_request().await;
        self.process_request(request).await;
    }

    async fn process_request(&self, request: Request) {
        match request {
            Request::CurTemp => {
                let temp = self.driver.lock().await.temperature().await.unwrap();
                self.device.send_response(Ok(Response::Ack(temp))).await;
            }
        }
    }
}

/// Should be called by a wrapper task per sensor (since tasks themselves cannot be generic)
pub async fn sensor_task<T: TemperatureSensor>(sensor: &'static Wrapper<T>) {
    loop {
        sensor.wait_and_process().await;
    }
}
