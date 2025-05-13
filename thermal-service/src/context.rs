//! Thermal service
use crate::fan;
use crate::mptf;
use crate::sensor;
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::once_lock::OnceLock;
use embedded_services::{comms, error, intrusive_list};

/// Error type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Error {
    /// The requested device does not exist
    InvalidDevice,
}

#[derive(Debug, Clone, Copy)]
pub struct ServiceRequest {
    pub msg: mptf::Request,
    pub from: comms::EndpointID,
}

impl ServiceRequest {
    pub fn new(msg: mptf::Request, from: comms::EndpointID) -> Self {
        Self { msg, from }
    }
}

struct Context {
    /// Registered temperature sensors
    sensors: intrusive_list::IntrusiveList,
    /// Registered fans
    fans: intrusive_list::IntrusiveList,
    /// Incoming request channel
    request: Channel<NoopRawMutex, ServiceRequest, 1>,
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
pub async fn register_sensor(sensor: &'static sensor::DeviceNode) -> Result<(), intrusive_list::Error> {
    if get_sensor(sensor.device.id()).await.is_some() {
        return Err(intrusive_list::Error::NodeAlreadyInList);
    }

    CONTEXT.get().await.sensors.push(sensor)
}

/// Register a fan with the thermal service
pub async fn register_fan(fan: &'static fan::DeviceNode) -> Result<(), intrusive_list::Error> {
    if get_fan(fan.device.id()).await.is_some() {
        return Err(intrusive_list::Error::NodeAlreadyInList);
    }

    CONTEXT.get().await.fans.push(fan)
}

/// Find a sensor by its ID
async fn get_sensor(id: sensor::DeviceId) -> Option<&'static sensor::Device> {
    for sensor in &CONTEXT.get().await.sensors {
        if let Some(data) = sensor.data::<sensor::DeviceNode>() {
            if data.device.id() == id {
                return Some(data.device);
            }
        } else {
            error!("Non-device located in sensors list");
        }
    }

    None
}

/// Find a fan by its ID
async fn get_fan(id: fan::DeviceId) -> Option<&'static fan::Device> {
    for fan in &CONTEXT.get().await.fans {
        if let Some(data) = fan.data::<fan::DeviceNode>() {
            if data.device.id() == id {
                return Some(data.device);
            }
        } else {
            error!("Non-device located in fan list");
        }
    }

    None
}

/// Singleton struct to give access to the thermal context
pub struct ContextToken(());
impl ContextToken {
    /// Create a new context token, returning None if this function has been called before
    pub fn create() -> Option<Self> {
        static INIT: AtomicBool = AtomicBool::new(false);
        if INIT.load(Ordering::SeqCst) {
            return None;
        }

        INIT.store(true, Ordering::SeqCst);
        Some(ContextToken(()))
    }

    /// Get a sensor by its ID
    pub async fn get_sensor(&self, id: sensor::DeviceId) -> Result<&'static sensor::Device, Error> {
        get_sensor(id).await.ok_or(Error::InvalidDevice)
    }

    /// Provides access to the sensors list
    pub async fn sensors(&self) -> &intrusive_list::IntrusiveList {
        &CONTEXT.get().await.sensors
    }

    /// Get a fan by its ID
    pub async fn get_fan(&self, id: fan::DeviceId) -> Result<&'static fan::Device, Error> {
        get_fan(id).await.ok_or(Error::InvalidDevice)
    }

    /// Provides access to the fans list
    pub async fn fans(&self) -> &intrusive_list::IntrusiveList {
        &CONTEXT.get().await.fans
    }

    // Process a received standard MPTF request
    pub(crate) async fn process_request(&self, _service_msg: mptf::Request) -> Result<mptf::Response, mptf::Error> {
        todo!()
    }

    pub(crate) async fn wait_request(&self) -> ServiceRequest {
        CONTEXT.get().await.request.receive().await
    }

    pub(crate) fn send_request_no_wait(&self, request: ServiceRequest) -> Result<(), ()> {
        CONTEXT
            .try_get()
            .ok_or(())?
            .request
            .try_send(ServiceRequest::new(request.msg, request.from))
            .map_err(|_| ())?;
        Ok(())
    }

    // Process a received standard MPTF request
    /*async fn process_mptf_request(&self, service_msg: mptf::Request) -> Result<mptf::Response, mptf::Error> {
        let tz = self.tz.lock().await;

        match service_msg {
            // Standard command
            mptf::Request::GetTmp(_) => tz.get_tmp().await,
            mptf::Request::GetThrs(_) => tz.get_thrs().await,
            mptf::Request::SetThrs(_, timeout, low, high) => tz.set_thrs(timeout, low, high).await,
            mptf::Request::SetScp(_, policy, acstc_limit, pow_limit) => {
                tz.set_scp(policy, acstc_limit, pow_limit).await
            }

            // DWORD Variable - Thermal
            mptf::Request::GetCrtTemp => tz.get_crt_temp().await,
            mptf::Request::SetCrtTemp(temp) => tz.set_crt_temp(temp).await,
            mptf::Request::GetProcHotTemp => tz.get_proc_hot_temp().await,
            mptf::Request::SetProcHotTemp(temp) => tz.set_proc_hot_temp(temp).await,
            mptf::Request::GetProfileType => tz.get_profile_type().await,
            mptf::Request::SetProfileType(profile_type) => tz.set_profile_type(profile_type).await,

            // DWORD Variable - Fan
            mptf::Request::GetFanOnTemp => tz.get_fan_on_temp().await,
            mptf::Request::SetFanOnTemp(temp) => tz.set_fan_on_temp(temp).await,
            mptf::Request::GetFanRampTemp => tz.get_fan_ramp_temp().await,
            mptf::Request::SetFanRampTemp(temp) => tz.set_fan_ramp_temp(temp).await,
            mptf::Request::GetFanMaxTemp => tz.get_fan_max_temp().await,
            mptf::Request::SetFanMaxTemp(temp) => tz.set_fan_max_temp(temp).await,
            mptf::Request::GetFanMinRpm => tz.get_fan_min_rpm().await,
            mptf::Request::GetFanMaxRpm => tz.get_fan_max_rpm().await,
            mptf::Request::GetFanCurrentRpm => tz.get_fan_current_rpm().await,

            // DWORD Variable - Fan Optional
            mptf::Request::GetFanMinDba => tz.get_fan_min_dba().await,
            mptf::Request::GetFanMaxDba => tz.get_fan_max_dba().await,
            mptf::Request::GetFanCurrentDba => tz.get_fan_current_dba().await,
            mptf::Request::GetFanMinSones => tz.get_fan_min_sones().await,
            mptf::Request::GetFanMaxSones => tz.get_fan_max_sones().await,
            mptf::Request::GetFanCurrentSones => tz.get_fan_current_sones().await,
        }
    }*/
}
