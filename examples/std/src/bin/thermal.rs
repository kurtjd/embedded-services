use embassy_executor::Spawner;
use embassy_sync::once_lock::OnceLock;
use embedded_fans_async as fan_trait;
use embedded_sensors_hal_async::{
    sensor as sensor_trait,
    temperature::{DegreesCelsius, TemperatureSensor},
};
use embedded_services::info;
use embedded_services::thermal::{self, fan, sensor};
use fan_trait::{Fan, RpmSense};
use thermal_service;

// A simple mock "transport" service for demo purposes
mod transport_service {
    use embassy_sync::once_lock::OnceLock;
    use embassy_time::Timer;
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
        // This simple service just assumes every message received is a response to a previous message sent
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

                // TODO: Handle other possible messages
                _ => Err(comms::MailboxDelegateError::InvalidData),
            }
        }
    }

    pub static TRANSPORT_SERVICE: OnceLock<Service> = OnceLock::new();

    pub async fn init() {
        let transport_service = TRANSPORT_SERVICE.get_or_init(|| Service::new());

        comms::register_endpoint(transport_service, &transport_service.endpoint)
            .await
            .unwrap();
    }

    // Demo simulating routing messages from the "host" to the thermal service
    #[embassy_executor::task]
    pub async fn transport_task() {
        init().await;
        let s = TRANSPORT_SERVICE.get().await;

        info!("Staring transport task");

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
}

// An example mock sensor with the embedded-sensors traits implemented
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

// An example mock fan with the embedded-fans traits implemented
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

// A task for each device
// Device tasks can use the generic device task or implement their own
#[embassy_executor::task]
async fn sensor_task_0(sensor: &'static sensor::Sensor<MockSensor>) {
    thermal_service::sensor_task(sensor).await;
}

#[embassy_executor::task]
async fn sensor_task_1(sensor: &'static sensor::Sensor<MockSensor>) {
    thermal_service::sensor_task(sensor).await;
}

#[embassy_executor::task]
async fn fan_task_0(fan: &'static fan::Fan<MockFan>) {
    thermal_service::fan_task(fan).await;
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    env_logger::builder().filter_level(log::LevelFilter::Info).init();
    embedded_services::init().await;

    spawner.must_spawn(thermal_service::thermal_service_task(spawner));
    spawner.must_spawn(transport_service::transport_task());

    // For each sensor and fan, we initialize the proper struct which we also pass a driver to
    // We then register it with the thermal service (which stores it in a linked list)
    // Finally we spawn a task for that device, which typically entails waiting for messages then acting

    info!("Creating sensor device 0");
    static SENSOR_0: OnceLock<sensor::Sensor<MockSensor>> = OnceLock::new();
    let sensor_0 = SENSOR_0.get_or_init(|| sensor::Sensor::new(thermal::DeviceId(0), MockSensor::new(50.0)));
    thermal::register_sensor(&sensor_0.device).await.unwrap();
    spawner.must_spawn(sensor_task_0(sensor_0));

    info!("Creating sensor device 1");
    static SENSOR_1: OnceLock<sensor::Sensor<MockSensor>> = OnceLock::new();
    let sensor_1 = SENSOR_1.get_or_init(|| sensor::Sensor::new(thermal::DeviceId(1), MockSensor::new(52.0)));
    thermal::register_sensor(&sensor_1.device).await.unwrap();
    spawner.must_spawn(sensor_task_1(sensor_1));

    info!("Creating fan device");
    static FAN: OnceLock<fan::Fan<MockFan>> = OnceLock::new();
    let fan = FAN.get_or_init(|| fan::Fan::new(thermal::DeviceId(0), MockFan::new()));
    thermal::register_fan(&fan.device).await.unwrap();
    spawner.must_spawn(fan_task_0(fan));
}
