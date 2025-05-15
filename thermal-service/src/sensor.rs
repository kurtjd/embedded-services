//! Sensor Device
use crate::utils::SampleBuf;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;
use embedded_sensors_hal_async::temperature::{TemperatureSensor, TemperatureThresholdWait};
use embedded_services::{intrusive_list, Node};

/// Sensor error type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// Invalid request
    InvalidRequest,
    /// Device encountered a hardware failure
    Hardware,
}

/// Sensor request
// TODO: More generic requests that make sense
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Request {
    /// Current temperature measurement
    GetTemp,
    /// Set low alert thresholds (in degrees Celsius)
    SetAlertLow(f32),
    /// Set high alert thresholds (in degrees Celsius)
    SetAlertHigh(f32),
    /// Set temperature sampling period (in ms)
    SetSamplingPeriod(u64),
}

/// Sensor response
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Response {
    /// Response for any request that is successful but does not require data
    Success,
    /// Current temperature of sensor in dC
    Temp(f32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Alert {
    /// High threshold was crossed
    ThresholdLow,
    /// Low threshold was crossed
    ThresholdHigh,
}

/// Device ID new type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct DeviceId(pub u8);

/// Sensor device struct
pub struct Device {
    /// Intrusive list node allowing Device to be contained in a list
    node: Node,
    /// Device ID
    id: DeviceId,
    /// Channel for requests to the device
    request: Channel<NoopRawMutex, Request, 1>,
    /// Channel for responses from the device
    response: Channel<NoopRawMutex, Result<Response, Error>, 1>,
    /// Channel for threshold alerts from this device
    alert: Channel<NoopRawMutex, Alert, 1>,
}

impl Device {
    /// Create a new sensor device
    pub fn new(id: DeviceId) -> Self {
        Self {
            node: Node::uninit(),
            id,
            request: Channel::new(),
            response: Channel::new(),
            alert: Channel::new(),
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

    pub async fn wait_alert(&self) -> Alert {
        self.alert.receive().await
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

// Internal sensor state
struct State {
    samples: SampleBuf<f32, 10>,
    period: u64,
    alert_low: f32,
    alert_high: f32,
}

impl Default for State {
    fn default() -> Self {
        Self {
            samples: SampleBuf::new(),
            period: 1000,
            alert_low: f32::MAX,
            alert_high: f32::MAX,
        }
    }
}

/// Wrapper binding a communication device, hardware driver, and additional state.
pub struct Sensor<T: TemperatureSensor + TemperatureThresholdWait> {
    /// Underlying device
    device: Device,
    /// Underlying driver
    driver: Mutex<NoopRawMutex, T>,
    /// Underlying sensor state
    state: Mutex<NoopRawMutex, State>,
}

impl<T: TemperatureSensor + TemperatureThresholdWait> Sensor<T> {
    /// New sensor wrapper
    pub fn new(id: DeviceId, controller: T) -> Self {
        Self {
            device: Device::new(id),
            driver: Mutex::new(controller),
            state: Mutex::new(State::default()),
        }
    }

    /// Process request for sensor
    pub async fn wait_and_process(&self) {
        let request = self.device.wait_request().await;
        let response = self.process_request(request).await;
        self.device.send_response(response).await;
    }

    // Processes request by making actual driver calls
    async fn process_request(&self, request: Request) -> Result<Response, Error> {
        match request {
            Request::GetTemp => {
                let temp = self.state.lock().await.samples.recent();
                Ok(Response::Temp(temp))
            }
            Request::SetAlertLow(low) => {
                self.driver
                    .lock()
                    .await
                    .set_temperature_threshold_low(low)
                    .await
                    .map_err(|_| Error::Hardware)?;

                self.state.lock().await.alert_low = low;
                Ok(Response::Success)
            }
            Request::SetAlertHigh(high) => {
                self.driver
                    .lock()
                    .await
                    .set_temperature_threshold_high(high)
                    .await
                    .map_err(|_| Error::Hardware)?;

                self.state.lock().await.alert_high = high;
                Ok(Response::Success)
            }
            Request::SetSamplingPeriod(period) => {
                self.state.lock().await.period = period;
                Ok(Response::Success)
            }
        }
    }
}

/// These should be called by a wrapper task per sensor (since tasks themselves cannot be generic, cannot define task here)
// TODO: Add macro to make that easier, and a macro that spawns all tasks
pub async fn rx_task<T: TemperatureSensor + TemperatureThresholdWait>(sensor: &'static Sensor<T>) {
    loop {
        sensor.wait_and_process().await;
    }
}

pub async fn sample_task<T: TemperatureSensor + TemperatureThresholdWait>(sensor: &'static Sensor<T>) {
    loop {
        if let Ok(temp) = sensor.driver.lock().await.temperature().await {
            sensor.state.lock().await.samples.push(temp);
        }

        Timer::after_millis(sensor.state.lock().await.period).await;
    }
}

pub async fn alert_task<T: TemperatureSensor + TemperatureThresholdWait>(sensor: &'static Sensor<T>) {
    loop {
        if let Ok(alert_temp) = sensor.driver.lock().await.wait_for_temperature_threshold().await {
            let alert = if alert_temp <= sensor.state.lock().await.alert_low {
                Alert::ThresholdLow
            } else {
                Alert::ThresholdHigh
            };

            sensor.device.alert.send(alert).await;
        }
    }
}
