use embassy_executor::Spawner;
use embassy_sync::once_lock::OnceLock;
use embassy_time::Timer;
use embedded_fans_async as fan_trait;
use embedded_sensors_hal_async::{
    sensor as sensor_trait,
    temperature::{DegreesCelsius, TemperatureSensor},
};
use embedded_services::ec_type::message::ThermalMessage;
use embedded_services::thermal::{self, fan, sensor};
use embedded_services::{comms, info};
use fan_trait::{Fan, RpmSense};
use std::cell::RefCell;
use thermal_service;

mod espi_service {
    use embassy_sync::once_lock::OnceLock;
    use embedded_services::comms::{self, EndpointID, External};
    use embedded_services::ec_type::message::ThermalMessage;
    use log::info;

    pub struct Service {
        pub endpoint: comms::Endpoint,
    }

    impl Service {
        pub fn new() -> Self {
            Service {
                endpoint: comms::Endpoint::uninit(EndpointID::External(External::Host)),
            }
        }
    }

    impl comms::MailboxDelegate for Service {
        fn receive(&self, message: &comms::Message) -> Result<(), comms::MailboxDelegateError> {
            let msg = message
                .data
                .get::<ThermalMessage>()
                .ok_or(comms::MailboxDelegateError::MessageNotFound)?;

            match msg {
                ThermalMessage::Fan1CurRpm(rpm) => {
                    info!("Fan current RPM: {}", rpm);
                    Ok(())
                }
                ThermalMessage::Fan1OnTemp(tmp) => {
                    info!("Fan on temp set to: {} *C", tmp);
                    Ok(())
                }
                ThermalMessage::Tmp1Val(tmp) => {
                    info!("Current average temperature: {} *C", tmp);
                    Ok(())
                }
                _ => Err(comms::MailboxDelegateError::InvalidData),
            }
        }
    }

    pub static ESPI_SERVICE: OnceLock<Service> = OnceLock::new();

    pub async fn init() {
        let espi_service = ESPI_SERVICE.get_or_init(|| Service::new());

        comms::register_endpoint(espi_service, &espi_service.endpoint)
            .await
            .unwrap();
    }
}

struct MockSensor {
    start_temp: f32,
}

impl MockSensor {
    fn new(start_temp: f32) -> Self {
        Self { start_temp }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MockSensorError;
impl sensor_trait::Error for MockSensorError {
    fn kind(&self) -> sensor_trait::ErrorKind {
        sensor_trait::ErrorKind::Other
    }
}

impl sensor_trait::ErrorType for MockSensor {
    type Error = MockSensorError;
}

impl TemperatureSensor for MockSensor {
    async fn temperature(&mut self) -> Result<DegreesCelsius, Self::Error> {
        self.start_temp += 5.0;
        Ok(self.start_temp)
    }
}

struct SensorDevice {
    device: thermal::sensor::Device,
    driver: RefCell<MockSensor>,
}

impl SensorDevice {
    fn new(id: thermal::DeviceId) -> Self {
        Self {
            device: thermal::sensor::Device::new(id),
            driver: RefCell::new(MockSensor::new(50.0)),
        }
    }

    async fn process_request(&self) {
        let request = self.device.wait_request().await;
        match request {
            sensor::Request::CurTemp => {
                let temp = self.driver.borrow_mut().temperature().await.unwrap();
                self.device.send_response(Ok(sensor::Response::Ack(temp))).await;
            }
        }
    }
}

struct MockFan {
    rpm: u32,
}

impl MockFan {
    fn new() -> Self {
        Self { rpm: 0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MockFanError;
impl fan_trait::Error for MockFanError {
    fn kind(&self) -> fan_trait::ErrorKind {
        fan_trait::ErrorKind::Other
    }
}

impl fan_trait::ErrorType for MockFan {
    type Error = MockFanError;
}

impl Fan for MockFan {
    fn max_rpm(&self) -> u16 {
        42000
    }

    fn min_rpm(&self) -> u16 {
        0
    }

    fn min_start_rpm(&self) -> u16 {
        100
    }

    async fn set_speed_rpm(&mut self, rpm: u16) -> Result<u16, Self::Error> {
        self.rpm = rpm as u32;
        Ok(rpm)
    }
}

impl RpmSense for MockFan {
    async fn rpm(&mut self) -> Result<u16, Self::Error> {
        Ok(self.rpm as u16)
    }
}

struct FanDevice {
    device: thermal::fan::Device,
    driver: RefCell<MockFan>,
}

impl FanDevice {
    fn new(id: thermal::DeviceId) -> Self {
        Self {
            device: thermal::fan::Device::new(id),
            driver: RefCell::new(MockFan::new()),
        }
    }

    async fn process_request(&self) {
        let request = self.device.wait_request().await;
        match request {
            fan::Request::CurRpm => {
                let rpm = self.driver.borrow_mut().rpm().await.unwrap();
                self.device.send_response(Ok(fan::Response::Ack(rpm as u32))).await;
            }
            fan::Request::SetRpm(rpm) => {
                self.driver.borrow_mut().set_speed_rpm(rpm as u16).await.unwrap();
                self.device.send_response(Ok(fan::Response::Ack(rpm))).await;
            }
        }
    }
}

#[embassy_executor::task]
async fn sensor_task(sensor: &'static SensorDevice) {
    loop {
        sensor.process_request().await;
    }
}

#[embassy_executor::task]
async fn fan_task(fan: &'static FanDevice) {
    loop {
        fan.process_request().await;
    }
}

#[embassy_executor::task]
async fn espi_task() {
    espi_service::init().await;
    let s = espi_service::ESPI_SERVICE.get().await;

    info!("Staring eSPI task");

    // Set fan on temperature
    Timer::after_millis(1000).await;
    s.endpoint
        .send(
            comms::EndpointID::Internal(comms::Internal::Thermal),
            &ThermalMessage::Fan1OnTemp(100),
        )
        .await
        .unwrap();
    Timer::after_millis(1000).await;

    loop {
        // Request average temperature
        s.endpoint
            .send(
                comms::EndpointID::Internal(comms::Internal::Thermal),
                &ThermalMessage::Tmp1Val(0),
            )
            .await
            .unwrap();
        Timer::after_millis(500).await;

        // Request current fan RPM
        s.endpoint
            .send(
                comms::EndpointID::Internal(comms::Internal::Thermal),
                &ThermalMessage::Fan1CurRpm(0),
            )
            .await
            .unwrap();
        Timer::after_millis(500).await;
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    env_logger::builder().filter_level(log::LevelFilter::Info).init();
    embedded_services::init().await;

    spawner.must_spawn(thermal_service::thermal_service_task(spawner));
    spawner.must_spawn(espi_task());

    info!("Creating sensor device");
    static SENSOR: OnceLock<SensorDevice> = OnceLock::new();
    let sensor = SENSOR.get_or_init(|| SensorDevice::new(thermal::DeviceId(0)));
    thermal::register_sensor(&sensor.device).await.unwrap();
    spawner.must_spawn(sensor_task(sensor));

    info!("Creating fan device");
    static FAN: OnceLock<FanDevice> = OnceLock::new();
    let fan = FAN.get_or_init(|| FanDevice::new(thermal::DeviceId(0)));
    thermal::register_fan(&fan.device).await.unwrap();
    spawner.must_spawn(fan_task(fan));
}
