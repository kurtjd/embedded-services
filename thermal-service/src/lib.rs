#![no_std]

pub use context::*;
use default_handler::DefaultHandler;
use embassy_sync::once_lock::OnceLock;
use embedded_services::{comms, error, info};

mod context;
pub mod default_handler;
pub mod fan;
pub mod mptf;
pub mod sensor;
mod utils;

struct Service<M: mptf::Handler> {
    context: context::ContextToken,
    endpoint: comms::Endpoint,
    mptf_handler: M,
}

impl<M: mptf::Handler> Service<M> {
    fn new(mptf_handler: M) -> Option<Self> {
        Some(Self {
            context: context::ContextToken::create()?,
            endpoint: comms::Endpoint::uninit(comms::EndpointID::Internal(comms::Internal::Thermal)),
            mptf_handler,
        })
    }

    async fn process_request(&self, request: mptf::Request) -> Result<mptf::Response, mptf::Error> {
        match request {
            mptf::Request::GetTmp(tzid) => self.mptf_handler.get_tmp(tzid).await,
            mptf::Request::GetThrs(tzid) => self.mptf_handler.get_thrs(tzid).await,
            mptf::Request::SetThrs(tzid, timeout, low, high) => {
                self.mptf_handler.set_thrs(tzid, timeout, low, high).await
            }
            mptf::Request::SetScp(tzid, mode, acoustic_lim, power_lim) => {
                self.mptf_handler.set_scp(tzid, mode, acoustic_lim, power_lim).await
            }
            mptf::Request::GetVar(uid) => self.mptf_handler.get_var(uid).await,
            mptf::Request::SetVar(uid, val) => self.mptf_handler.set_var(uid, val).await,
        }
    }

    async fn wait_and_process(&self) {
        let message = self.context.wait_request().await;
        let response = self.process_request(message.request).await;
        self.endpoint.send(message.from, &response).await.unwrap()
    }
}

impl<M: mptf::Handler> comms::MailboxDelegate for Service<M> {
    fn receive(&self, message: &comms::Message) -> Result<(), comms::MailboxDelegateError> {
        // This method gets called by embedded-services any time a message is sent to this service.
        // We check if its a standard MPTF request, and if so, handle it ourselves.
        // Otherwise return error.
        if let Some(&msg) = message.data.get::<mptf::Request>() {
            self.context
                .send_message_no_wait(context::Message {
                    request: msg,
                    from: message.from,
                })
                .map_err(|_| comms::MailboxDelegateError::BufferFull)
        } else {
            Err(comms::MailboxDelegateError::InvalidData)
        }
    }
}

// Just one instance of the service should be running
// TODO: Use Macro to make this all generic over MPTF Handler
static SERVICE: OnceLock<Service<DefaultHandler>> = OnceLock::new();

// This task exists solely to listen for incoming requests and then process them appropriately
#[embassy_executor::task]
async fn rx_task() {
    let service = SERVICE.get().await;

    loop {
        service.wait_and_process().await;
    }
}

/// This must be called to initialize the Thermal service and spawn additional tasks
pub async fn init(spawner: embassy_executor::Spawner, mptf_handler: DefaultHandler) {
    info!("Starting thermal service task");
    context::init();
    let service =
        SERVICE.get_or_init(|| Service::new(mptf_handler).expect("Thermal service singleton already initialized"));

    if comms::register_endpoint(service, &service.endpoint).await.is_err() {
        error!("Failed to register thermal service endpoint");
        return;
    }

    // Spawn the task for receiving thermal messages
    spawner.must_spawn(rx_task());

    // Spawn the task for MPTF handler
    // TODO: Make sure to spawn OEM task if default not used
    spawner.must_spawn(default_handler::task(spawner));
}

/// Send and wait for a request to complete, as opposed to sending through comms service.
pub async fn execute_request(request: mptf::Request) -> Result<mptf::Response, mptf::Error> {
    let service = SERVICE.get().await;
    service
        .context
        .send_message(Message::new(
            request,
            comms::EndpointID::External(comms::External::Host),
        ))
        .await;
    let message = service.context.wait_request().await;
    service.process_request(message.request).await
}

/// Used to send messages to other services from the Thermal service,
/// such as notifying the Power service if CRT TEMP is reached.
pub async fn send_service_msg(to: comms::EndpointID, data: &impl embedded_services::Any) {
    let service = SERVICE.get().await;
    service.endpoint.send(to, data).await.expect("Infallible");
}
