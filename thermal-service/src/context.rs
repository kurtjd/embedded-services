//! Thermal service
use crate::fan;
use crate::mptf;
use crate::sensor;
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::once_lock::OnceLock;
use embedded_services::{comms, error, intrusive_list};

#[derive(Debug, Clone, Copy)]
pub struct Message {
    pub request: mptf::Request,
    pub from: comms::EndpointID,
}

impl Message {
    pub fn new(request: mptf::Request, from: comms::EndpointID) -> Self {
        Self { request, from }
    }
}

struct Context {
    /// Registered temperature sensors
    sensors: intrusive_list::IntrusiveList,
    /// Registered fans
    fans: intrusive_list::IntrusiveList,
    /// Incoming request channel
    request: Channel<NoopRawMutex, Message, 1>,
}

impl Context {
    fn new() -> Self {
        Self {
            sensors: intrusive_list::IntrusiveList::new(),
            fans: intrusive_list::IntrusiveList::new(),
            request: Channel::new(),
        }
    }
}

static CONTEXT: OnceLock<Context> = OnceLock::new();

/// Init thermal service context
pub fn init() {
    CONTEXT.get_or_init(Context::new);
}

/// Register a sensor with the thermal service
pub async fn register_sensor(sensor: &'static sensor::Device) -> Result<(), intrusive_list::Error> {
    if get_sensor(sensor.id()).await.is_some() {
        return Err(intrusive_list::Error::NodeAlreadyInList);
    }

    CONTEXT.get().await.sensors.push(sensor)
}

/// Provides access to the sensors list
pub async fn sensors() -> &'static intrusive_list::IntrusiveList {
    &CONTEXT.get().await.sensors
}

/// Find a sensor by its ID
pub async fn get_sensor(id: sensor::DeviceId) -> Option<&'static sensor::Device> {
    for sensor in &CONTEXT.get().await.sensors {
        if let Some(data) = sensor.data::<sensor::Device>() {
            if data.id() == id {
                return Some(data);
            }
        } else {
            error!("Non-device located in sensors list");
        }
    }

    None
}

/// Send a request to a sensor through the thermal service instead of directly.
pub async fn execute_sensor_request(
    id: sensor::DeviceId,
    request: sensor::Request,
) -> Result<sensor::Response, sensor::Error> {
    let sensor = get_sensor(id).await.ok_or(sensor::Error::InvalidRequest)?;
    sensor.execute_request(request).await
}

/// Register a fan with the thermal service
pub async fn register_fan(fan: &'static fan::Device) -> Result<(), intrusive_list::Error> {
    if get_fan(fan.id()).await.is_some() {
        return Err(intrusive_list::Error::NodeAlreadyInList);
    }

    CONTEXT.get().await.fans.push(fan)
}

/// Provides access to the fans list
pub async fn fans() -> &'static intrusive_list::IntrusiveList {
    &CONTEXT.get().await.fans
}

/// Find a fan by its ID
pub async fn get_fan(id: fan::DeviceId) -> Option<&'static fan::Device> {
    for fan in &CONTEXT.get().await.fans {
        if let Some(data) = fan.data::<fan::Device>() {
            if data.id() == id {
                return Some(data);
            }
        } else {
            error!("Non-device located in fan list");
        }
    }

    None
}

/// Send a request to a fan through the thermal service instead of directly.
pub async fn execute_fan_request(id: fan::DeviceId, request: fan::Request) -> Result<fan::Response, fan::Error> {
    let fan = get_fan(id).await.ok_or(fan::Error::InvalidRequest)?;
    fan.execute_request(request).await
}

/// Singleton struct to give access to the thermal context
pub(crate) struct ContextToken(());
impl ContextToken {
    /// Create a new context token, returning None if this function has been called before
    pub(crate) fn create() -> Option<Self> {
        static INIT: AtomicBool = AtomicBool::new(false);
        if INIT.load(Ordering::SeqCst) {
            return None;
        }

        INIT.store(true, Ordering::SeqCst);
        Some(ContextToken(()))
    }

    pub(crate) async fn wait_request(&self) -> Message {
        CONTEXT.get().await.request.receive().await
    }

    pub(crate) async fn send_message(&self, msg: Message) {
        CONTEXT
            .get()
            .await
            .request
            .send(Message::new(msg.request, msg.from))
            .await
    }

    pub(crate) fn send_message_no_wait(&self, msg: Message) -> Result<(), ()> {
        CONTEXT.try_get().ok_or(())?.request.try_send(msg).map_err(|_| ())?;
        Ok(())
    }
}
