#![no_std]

pub use context::*;
use embassy_sync::once_lock::OnceLock;
use embedded_services::{comms, error, info};

pub mod context;
pub mod fan;
pub mod mptf;
pub mod sensor;

pub struct Service {
    context: context::ContextToken,
    endpoint: comms::Endpoint,
}

impl Service {
    fn create() -> Option<Self> {
        Some(Self {
            context: context::ContextToken::create()?,
            endpoint: comms::Endpoint::uninit(comms::EndpointID::Internal(comms::Internal::Thermal)),
        })
    }

    async fn wait_and_process(&self) {
        let request = self.context.wait_request().await;
        let response = self.context.process_request(request.msg).await;
        self.endpoint.send(request.from, &response).await.unwrap()
    }
}

impl comms::MailboxDelegate for Service {
    fn receive(&self, message: &comms::Message) -> Result<(), comms::MailboxDelegateError> {
        // This method gets called by embedded-services any time a message is sent to this service.
        // We check if its a standard MPTF request, and if so, handle it ourselves.
        // Otherwise return error.
        if let Some(&msg) = message.data.get::<mptf::Request>() {
            self.context
                .send_request_no_wait(context::ServiceRequest {
                    msg,
                    from: message.from,
                })
                .map_err(|_| comms::MailboxDelegateError::BufferFull)
        } else {
            Err(comms::MailboxDelegateError::InvalidData)
        }
    }
}

// Just one instance of the service should be running
static SERVICE: OnceLock<Service> = OnceLock::new();

// This task exists solely to listen for incoming requests and then process them appropriately
#[embassy_executor::task]
pub async fn rx_task() {
    let s = SERVICE.get().await;

    loop {
        s.wait_and_process().await;
    }
}

/// This must be called to initialize the Thermal service and spawn additional tasks
pub async fn init(spawner: embassy_executor::Spawner) {
    info!("Starting thermal service task");
    let service = SERVICE.get_or_init(|| Service::create().expect("Thermal service singleton already initialized"));

    if comms::register_endpoint(service, &service.endpoint).await.is_err() {
        error!("Failed to register thermal service endpoint");
        return;
    }

    // But always spawn the task for receiving thermal messages
    spawner.must_spawn(rx_task());
}

/// Send a request to a sensor through the thermal service instead of directly.
pub async fn execute_sensor_request(
    id: sensor::DeviceId,
    request: sensor::Request,
) -> Result<sensor::Response, sensor::Error> {
    let service = SERVICE.get().await;
    let sensor = service
        .context
        .get_sensor(id)
        .await
        .map_err(|_| sensor::Error::InvalidRequest)?;
    sensor.execute_request(request).await
}

/// Send a request to a fan through the thermal service instead of directly.
pub async fn execute_fan_request(id: fan::DeviceId, request: fan::Request) -> Result<fan::Response, fan::Error> {
    let service = SERVICE.get().await;
    let fan = service
        .context
        .get_fan(id)
        .await
        .map_err(|_| fan::Error::InvalidRequest)?;
    fan.execute_request(request).await
}
