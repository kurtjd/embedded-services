#![no_std]

use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::mutex::Mutex;
use embassy_sync::once_lock::OnceLock;
use embassy_time::Timer;
use embedded_services::ec_type::message::ThermalMessage;
use embedded_services::ecs::{Component, EntityRef, Layer};
use embedded_services::{comms, error, info};

pub mod context;
pub mod fan;
pub mod sensor;
pub use context::*;

struct InternalState {
    fan_on_temp: f32,
    average_temp: f32,
}

impl InternalState {
    fn new() -> Self {
        Self {
            fan_on_temp: 10000.0,
            average_temp: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ServiceMsg {
    msg: ThermalMessage,
    from: comms::EndpointID,
}

pub struct ThermalService {
    context: context::ContextToken,
    endpoint: comms::Endpoint,
    request: Channel<NoopRawMutex, ServiceMsg, 1>,
    state: Mutex<NoopRawMutex, InternalState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThermalServiceErrors {
    InvalidRequest,
    Unknowm,
}

impl ThermalService {
    pub fn create() -> Option<Self> {
        context::init();
        Some(Self {
            context: context::ContextToken::create()?,
            endpoint: comms::Endpoint::uninit(comms::EndpointID::Internal(comms::Internal::Thermal)),
            request: Channel::new(),
            state: Mutex::new(InternalState::new()),
        })
    }

    // Measure average temperature of all registered sensors
    async fn measure_average_temp(&self) -> f32 {
        // TODO: Make Rustier, just temporary throwaway code
        let mut sum = 0.0;
        let nsensors = self.context.sensors().await.into_iter().count() as f32;
        for sensor in self.context.sensors().await {
            if let Some(sensor) = sensor.data::<sensor::Device>() {
                match sensor.execute_request(sensor::Request::CurTemp).await {
                    Ok(sensor::Response::Ack(temp)) => sum += temp,
                    _ => todo!("Handle error or other odd response"),
                };
            }
        }

        sum / nsensors
    }

    // Process a received service message
    async fn process_service_msg(&self, service_msg: &ServiceMsg) -> Result<ThermalMessage, ThermalServiceErrors> {
        match service_msg.msg {
            // For now interpret this as get average temperature of all sensors
            ThermalMessage::Tmp1Val(_) => Ok(ThermalMessage::Tmp1Val(self.state.lock().await.average_temp as u32)),

            // For now interpret this as the temperature point the fans will kick in
            ThermalMessage::Fan1OnTemp(tmp) => {
                // Update our state for the temp monitoring task to check
                self.state.lock().await.fan_on_temp = tmp as f32;
                // Just echo tmp as response for now
                Ok(ThermalMessage::Fan1OnTemp(tmp))
            }

            // For now interpret this as just the RPM of the first fan in list
            ThermalMessage::Fan1CurRpm(_) => {
                if let Some(fan) = self
                    .context
                    .fans()
                    .await
                    .into_iter()
                    .next()
                    .unwrap()
                    .data::<fan::Device>()
                {
                    let rpm = match fan.execute_request(fan::Request::CurRpm).await {
                        Ok(fan::Response::Ack(rpm)) => rpm,
                        _ => todo!("Handle error or other odd response"),
                    };
                    Ok(ThermalMessage::Fan1CurRpm(rpm))
                } else {
                    Err(ThermalServiceErrors::Unknowm)
                }
            }

            // TODO: Handle other possible requests
            _ => Err(ThermalServiceErrors::InvalidRequest),
        }
    }
}

impl comms::MailboxDelegate for ThermalService {
    fn receive(&self, message: &comms::Message) -> Result<(), comms::MailboxDelegateError> {
        // If standard message, send to our request channel for processing
        if let Some(&mptf_msg) = message.data.get::<ThermalMessage>() {
            self.request
                .try_send(ServiceMsg {
                    msg: mptf_msg,
                    from: message.from,
                })
                .map_err(|_| comms::MailboxDelegateError::BufferFull)
        } else {
            // TODO: Still need to figure out handling non-standard/OEM messages
            Err(comms::MailboxDelegateError::MessageNotFound)
        }
    }
}

struct GenericMessageBridge;
impl Component<&ThermalService> for GenericMessageBridge {
    type Event = ServiceMsg;

    async fn wait_event(&self, entity: &&ThermalService) -> Self::Event {
        entity.request.receive().await
    }

    async fn process(&self, entity: &mut &ThermalService, event: Self::Event) -> () {
        let response = entity.process_service_msg(&event).await.unwrap();

        // Send a response back to service that sent the initial request
        entity.endpoint.send(event.from, &response).await.unwrap();
    }
}

static SERVICE: OnceLock<ThermalService> = OnceLock::new();

#[embassy_executor::task]
pub async fn service_rx_task() {
    let mut s = EntityRef::new(SERVICE.get().await).add_component(GenericMessageBridge);

    loop {
        s.process_all().await;
    }
}

#[embassy_executor::task]
pub async fn thermal_service_task(spawner: embassy_executor::Spawner) {
    info!("Starting thermal service task");
    let service =
        SERVICE.get_or_init(|| ThermalService::create().expect("Thermal service singleton already initialized"));

    if comms::register_endpoint(service, &service.endpoint).await.is_err() {
        error!("Failed to register thermal service endpoint");
        return;
    }

    // Spawn additional tasks
    spawner.must_spawn(service_rx_task());

    // Handle overall thermal-service logic
    // Would be broken into different tasks if makes sense
    loop {
        // For now, basically just periodically measure the average temperature and cache it.
        {
            let mut state = service.state.lock().await;
            state.average_temp = service.measure_average_temp().await;

            // Also check if measured average temp is greater than fan on temp, and if so, start it
            // This logic is very flawed but you get the gist
            if state.average_temp > state.fan_on_temp {
                if let Some(fan) = service
                    .context
                    .fans()
                    .await
                    .into_iter()
                    .next()
                    .unwrap()
                    .data::<fan::Device>()
                {
                    fan.execute_request(fan::Request::SetRpm(1337)).await.unwrap();
                }
            }
        }

        // Just wait some time
        Timer::after_millis(1000).await;
    }
}
