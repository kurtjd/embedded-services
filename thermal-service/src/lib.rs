#![no_std]

use embassy_futures::select::select;
use embassy_futures::select::Either::{First, Second};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embedded_fans_async::{Fan, RpmSense};
use embedded_sensors_hal_async::temperature::{TemperatureSensor, TemperatureThresholdWait};
use embedded_services::comms;
use embedded_services::ec_type::message::ThermalMessage;

mod fan;
mod temperature;

/// Example OEM messages.
/// True OEM messages should exist in OEM service, this is just an example.
#[derive(Copy, Clone, Debug)]
pub enum OemMessage {
    Temp1(u8),
    Temp2(u8),
}

/// Generic to hold OEM messages and standard ACPI messages
/// Can add more as more services have messages
#[derive(Copy, Clone, Debug)]
pub enum ThermalMsgs {
    Acpi(ThermalMessage),
    Oem(OemMessage),
}

/// Thermal Service Errors
#[derive(Copy, Clone, Debug)]
pub enum ThermalServiceErrors {
    BufferFull,
    ThermalErr1,
    ThermalErr2,
}

#[derive(Default)]
struct InternalState {
    // TODO
    on_temp: u8,
    max_temp: u8,
    temp_cache: [u8; 10],
}

pub struct Service<T: TemperatureSensor + TemperatureThresholdWait, F: Fan + RpmSense> {
    pub endpoint: comms::Endpoint,
    pub temperature_sensor: temperature::TemperatureSensorDevice<T>,
    pub fan: fan::FanDevice<F>,
    state: Mutex<NoopRawMutex, InternalState>,
}

impl<T: TemperatureSensor + TemperatureThresholdWait, F: Fan + RpmSense> Service<T, F> {
    pub fn new(temperature_sensor: T, fan: F) -> Self {
        Self {
            endpoint: comms::Endpoint::uninit(comms::EndpointID::Internal(comms::Internal::Thermal)),
            temperature_sensor: temperature::TemperatureSensorDevice::new(temperature_sensor),
            fan: fan::FanDevice::new(fan),
            state: Mutex::new(InternalState::default()),
        }
    }

    /// Function to read temperature data and control fans according to parameters.
    /// Intended to be called within a timer callback context, to periodically update state and perform logic.
    /// Would also periodically send temperature readings to espi service for espi to cache?
    pub async fn update(&self) {
        todo!();
    }

    /// Function that handles responses from the temperature sensor and fan and sends messages over the endpoint.
    pub async fn handle_response(&self) -> Result<(), ThermalServiceErrors> {
        let temperature_fut = self.temperature_sensor.tx.receive();
        let fan_fut = self.fan.tx.receive();

        let msg = match select(temperature_fut, fan_fut).await {
            First(res) => match res {
                Ok(msg) => msg,
                Err(e) => match e {
                    temperature::TemperatureSensorError::Bus => return Err(ThermalServiceErrors::ThermalErr1),
                },
            },
            Second(res) => match res {
                Ok(msg) => msg,
                Err(e) => match e {
                    fan::FanError::Bus => return Err(ThermalServiceErrors::ThermalErr2),
                },
            },
        };

        // Route the response over the comms service to the external host.
        // In this context, does 'host' represent the transport layer service?
        // So like I see the espi service registers itself as the Host endpoint
        // I suppose the SSH service can do this to
        // And this allows the thermal-service to be transport agnostic?
        match msg {
            ThermalMsgs::Acpi(msg) => {
                self.endpoint
                    .send(comms::EndpointID::External(comms::External::Host), &msg)
                    .await
                    .unwrap();
            }
            ThermalMsgs::Oem(_) => todo!(),
        }

        Ok(())
    }

    fn handle_request(&self, msg: ThermalMsgs) -> Result<(), ThermalServiceErrors> {
        match msg {
            ThermalMsgs::Acpi(acpi_msg) => match acpi_msg {
                // Handled directly by the service
                // Would involve managing internal state and sending additional messages to fan
                ThermalMessage::Events(_) => todo!(),
                ThermalMessage::CoolMode(_) => todo!(),
                ThermalMessage::Fan1OnTemp(_) => todo!(),
                ThermalMessage::Fan1RampTemp(_) => todo!(),
                ThermalMessage::Fan1MaxTemp(_) => todo!(),
                ThermalMessage::Fan1CrtTemp(_) => todo!(),
                ThermalMessage::Fan1HotTemp(_) => todo!(),

                // Route these to the fan device to handle
                ThermalMessage::Fan1CurRpm(_)
                | ThermalMessage::Fan1MaxRpm(_)
                | ThermalMessage::DbaLimit(_)
                | ThermalMessage::SonneLimit(_)
                | ThermalMessage::MaLimit(_) => self.fan.rx.try_send(msg).map_err(|_| ThermalServiceErrors::BufferFull),

                // Route these to the temperature sensor device to handle
                ThermalMessage::Tmp1Val(_)
                | ThermalMessage::Tmp1Low(_)
                | ThermalMessage::Tmp1High(_)
                | ThermalMessage::Tmp1Timeout(_) => self
                    .temperature_sensor
                    .rx
                    .try_send(msg)
                    .map_err(|_| ThermalServiceErrors::BufferFull),
            },
            ThermalMsgs::Oem(_oem_msg) => todo!(),
            // TODO: Would want to forward to OEM service (using appropriate OEM key)
            /*ThermalMsgs::Oem(oem_msg) => self
            .endpoint
            .send(comms::EndpointID::External(comms::External::Oem(OEM_KEY)), &oem_msg)
            .await
            .map_err(|_| ThermalServiceErrors::BufferFull),*/
        }
    }
}

impl<TemperatureSensorDevice: TemperatureSensor + TemperatureThresholdWait, FanDevice: Fan + RpmSense>
    comms::MailboxDelegate for Service<TemperatureSensorDevice, FanDevice>
{
    // Waits for a request to appear in the Thermal service's mailbox, then routes or handles it appropriately.
    fn receive(&self, message: &comms::Message) -> Result<(), comms::MailboxDelegateError> {
        if let Some(msg) = message.data.get::<ThermalMessage>() {
            self.handle_request(ThermalMsgs::Acpi(*msg)).map_err(|e| match e {
                ThermalServiceErrors::BufferFull => comms::MailboxDelegateError::BufferFull,
                _ => comms::MailboxDelegateError::Other,
            })?;
        } else if let Some(msg) = message.data.get::<OemMessage>() {
            self.handle_request(ThermalMsgs::Oem(*msg)).map_err(|e| match e {
                ThermalServiceErrors::BufferFull => comms::MailboxDelegateError::BufferFull,
                _ => comms::MailboxDelegateError::Other,
            })?;
        } else {
            return Err(comms::MailboxDelegateError::MessageNotFound);
        }

        Ok(())
    }
}

/// Generates the service instance and
///
/// - thermal_service_init()
/// - thermal_service_task()
/// - temperature_sensor_task()
/// - fan_task()
#[macro_export]
macro_rules! create_thermal_service {
    ($temperature_sensor:ident, $temperature_sensor_bus:path, $fan:ident, $fan_bus:path) => {
        use ::embedded_services::{comms, error};
        use ::thermal_service::{Service, ThermalServiceErrors};

        // TODO: Consider having this take an already instantiated device
        static SERVICE: OnceLock<Service<$temperature_sensor<$temperature_sensor_bus>, $fan<$fan_bus>>> =
            OnceLock::new();

        // Initializes the service and registers the service as a Thermal endpoint.
        pub async fn thermal_service_init(tmp_bus: $temperature_sensor_bus, fan_bus: $fan_bus) {
            let thermal_service =
                SERVICE.get_or_init(|| Service::new($temperature_sensor::new(tmp_bus), $fan::new(fan_bus)));

            comms::register_endpoint(thermal_service, &thermal_service.endpoint)
                .await
                .unwrap();
        }

        // This task spawns additional tasks for the temp sensor and fan tasks, as well as the update task.
        // It then waits for responses to be generated and processes them.
        #[embassy_executor::task]
        async fn thermal_service_task(spawner: Spawner) {
            // Block until service is initialized
            let s = SERVICE.get().await;

            spawner.must_spawn(temperature_sensor_task());
            spawner.must_spawn(fan_task());
            spawner.must_spawn(update_task());

            loop {
                if let Err(e) = s.handle_response().await {
                    match e {
                        _ => error!("Couldn't process response."),
                    }
                }
            }
        }

        // This task periodically performs higher-level logic and updates state.
        // It requires a timer, but should timer be handled elsewhere?
        #[embassy_executor::task]
        async fn update_task() {
            // Block until service is initialized
            let s = SERVICE.get().await;

            loop {
                s.update().await;
                embassy_time::Timer::after_secs(1).await;
            }
        }

        // This task waits for the temperature sensor to receive a message, then processes it.
        #[embassy_executor::task]
        async fn temperature_sensor_task() {
            // Block until service is initialized
            let s = SERVICE.get().await;

            loop {
                s.temperature_sensor.process_service_message().await;
            }
        }

        // This task waits for the fan to receive a message, then processes it.
        #[embassy_executor::task]
        async fn fan_task() {
            // Block until service is initialized
            let s = SERVICE.get().await;

            loop {
                s.fan.process_service_message().await;
            }
        }
    };
}
