//! Fan Device
use crate::utils::SampleBuf;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;
use embedded_fans_async as fan_traits;
use embedded_services::{intrusive_list, Node};

/// Fan error type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// Invalid request
    InvalidRequest,
    /// Device encountered a hardware failure
    Hardware,
}

/// Fan request
// TODO: More generic requests that make sense
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Request {
    /// Get RPM
    GetRpm,
    /// Get Min RPM
    GetMinRpm,
    /// Get Max RPM
    GetMaxRpm,
    /// Set RPM
    SetRpm(u16),
    /// Set RPM sampling period (in ms)
    SetSamplingPeriod(u64),
}

/// Fan response
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Response {
    /// Response for any request that is successful but does not require data
    Success,
    /// Current RPM
    Rpm(u16),
    /// Min RPM
    MinRpm(u16),
    /// Max RPM
    MaxRpm(u16),
}

/// Device ID new type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct DeviceId(pub u8);

/// Fan device struct
pub struct Device {
    /// Intrusive list node allowing Device to be contained in a list
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

    /// Execute request and wait for response
    pub async fn execute_request(&self, request: Request) -> Result<Response, Error> {
        self.request.send(request).await;
        self.response.receive().await
    }

    /// Wait for a request
    async fn wait_request(&self) -> Request {
        self.request.receive().await
    }

    /// Send a response
    async fn send_response(&self, response: Result<Response, Error>) {
        self.response.send(response).await;
    }
}

impl intrusive_list::NodeContainer for Device {
    fn get_node(&self) -> &Node {
        &self.node
    }
}

// Internal fan state
struct State {
    samples: SampleBuf<u16, 10>,
    period: u64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            samples: SampleBuf::new(),
            period: 1000,
        }
    }
}

/// Fan struct containing device for comms and driver
pub struct Fan<T: fan_traits::Fan + fan_traits::RpmSense> {
    /// Underlying device
    device: Device,
    /// Underlying driver
    driver: Mutex<NoopRawMutex, T>,
    /// Underlying fan state
    state: Mutex<NoopRawMutex, State>,
}

impl<T: fan_traits::Fan + fan_traits::RpmSense> Fan<T> {
    /// New fan
    pub fn new(id: DeviceId, driver: T) -> Self {
        Self {
            device: Device::new(id),
            driver: Mutex::new(driver),
            state: Mutex::new(State::default()),
        }
    }

    /// Process request for fan
    pub async fn wait_and_process(&self) {
        let request = self.device.wait_request().await;
        let response = self.process_request(request).await;
        self.device.send_response(response).await;
    }

    async fn process_request(&self, request: Request) -> Result<Response, Error> {
        match request {
            Request::GetRpm => {
                let rpm = self.state.lock().await.samples.recent();
                Ok(Response::Rpm(rpm))
            }
            Request::SetRpm(rpm) => {
                self.driver
                    .lock()
                    .await
                    .set_speed_rpm(rpm)
                    .await
                    .map_err(|_| Error::Hardware)?;
                Ok(Response::Success)
            }
            Request::GetMinRpm => {
                let min_rpm = self.driver.lock().await.min_rpm();
                Ok(Response::MinRpm(min_rpm))
            }
            Request::GetMaxRpm => {
                let max_rpm = self.driver.lock().await.max_rpm();
                Ok(Response::MaxRpm(max_rpm))
            }
            Request::SetSamplingPeriod(period) => {
                self.state.lock().await.period = period;
                Ok(Response::Success)
            }
        }
    }
}

/// Should be called by a wrapper task per fan (since tasks themselves cannot be generic)
// TODO: Add macro to make that easier, and a macro that spawns all tasks
pub async fn rx_task<T: fan_traits::Fan + fan_traits::RpmSense>(fan: &'static Fan<T>) {
    loop {
        fan.wait_and_process().await;
    }
}

pub async fn sample_task<T: fan_traits::Fan + fan_traits::RpmSense>(fan: &'static Fan<T>) {
    loop {
        if let Ok(rpm) = fan.driver.lock().await.rpm().await {
            fan.state.lock().await.samples.push(rpm);
        }

        Timer::after_millis(fan.state.lock().await.period).await;
    }
}
