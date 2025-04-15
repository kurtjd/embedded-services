//! Fan Device
use super::DeviceId;
use crate::intrusive_list;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;

/// Fan error type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// An unknown error occurred
    Unknown,
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
