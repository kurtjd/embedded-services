use crate as thermal_service;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::Timer;
use embedded_services::comms;
use embedded_services::error;
use embedded_services::info;
use heapless::FnvIndexMap;
use thermal_service::fan;
use thermal_service::mptf;
use thermal_service::sensor;
use thermal_service::utils;

mod uid {
    use crate::mptf::Uid;
    pub(crate) const CRT_TEMP: Uid = "218246e7-baf6-45f1-aa13-07e4845256b8";
    pub(crate) const PROC_HOT_TEMP: Uid = "22dc52d2-fd0b-47ab-95b8-26552f9831a5";
    pub(crate) const PROFILE_TYPE: Uid = "23b4a025-cdfd-4af9-a411-37a24c574615";
    pub(crate) const FAN_ON_TEMP: Uid = "ba17b567-c368-48d5-bc6f-a312a41583c1";
    pub(crate) const FAN_RAMP_TEMP: Uid = "3a62688c-d95b-4d2d-bacc-90d7a5816bcd";
    pub(crate) const FAN_MAX_TEMP: Uid = "dcb758b1-f0fd-4ec7-b2c0-ef1e2a547b76";
    pub(crate) const FAN_MIN_RPM: Uid = "db261c77-934b-45e2-9742-256c62badb7a";
    pub(crate) const FAN_MAX_RPM: Uid = "5cf839df-8be7-42b9-9ac5-3403ca2c8a6a";
    pub(crate) const FAN_CURRENT_RPM: Uid = "adf95492-0776-4ffc-84f3-b6c8b5269683";
}

enum FanState {
    Off,
    On,
    Ramping,
    Max,
}

//
struct State {
    // Cached, most recently measured temperature
    cur_temp: f32,
    // Current fan state
    fan_state: FanState,

    // Cached values from standard MPTF inputs
    thresholds: (u32, f32, f32),
    cooling_policy: u32,
    acoustic_lim: u32,
    power_lim: u32,

    // Cached values from MPTF SetVar inputs
    // Helps make it obvious we are affecting state based on MPTF variables via UID lookup
    uid_table: FnvIndexMap<&'static str, mptf::Dword, 9>,
}

impl Default for State {
    fn default() -> Self {
        let mut uid_table = FnvIndexMap::new();

        // TODO: Better defaults

        // 90 deg C = 3,631 dK
        uid_table.insert(uid::CRT_TEMP, 3631).unwrap();

        // 100 deg C = 3,731 dK
        uid_table.insert(uid::PROC_HOT_TEMP, 3731).unwrap();

        // Maybe unused by default?
        uid_table.insert(uid::PROFILE_TYPE, 0).unwrap();

        // 70 deg C = 3431 dK
        uid_table.insert(uid::FAN_ON_TEMP, 3431).unwrap();

        // 75 deg C = 3481 dK
        uid_table.insert(uid::FAN_RAMP_TEMP, 3481).unwrap();

        // 80 deg C = 3531 dK
        uid_table.insert(uid::FAN_MAX_TEMP, 3531).unwrap();

        // TODO: Default to actual min RPM from hardware?
        uid_table.insert(uid::FAN_MIN_RPM, 0).unwrap();

        // TODO: Default to actual max RPM from hardware?
        uid_table.insert(uid::FAN_MAX_RPM, 0).unwrap();

        // TODO: Does it make sense just to default to 0?
        uid_table.insert(uid::FAN_CURRENT_RPM, 0).unwrap();

        Self {
            cur_temp: 0.0,
            fan_state: FanState::Off,

            thresholds: (0, 0.0, 0.0),
            cooling_policy: 0,
            acoustic_lim: 0,
            power_lim: 0,

            uid_table,
        }
    }
}

pub struct DefaultHandler {
    state: Mutex<NoopRawMutex, State>,
}

impl DefaultHandler {
    async fn ramp_response(&self, temp: f32) -> Result<(), ()> {
        let min_rpm = match thermal_service::execute_fan_request(fan::DeviceId(0), fan::Request::GetMinRpm).await {
            Ok(fan::Response::MinRpm(rpm)) => rpm,
            _ => return Err(()),
        };
        let max_rpm = match thermal_service::execute_fan_request(fan::DeviceId(0), fan::Request::GetMaxRpm).await {
            Ok(fan::Response::MaxRpm(rpm)) => rpm,
            _ => return Err(()),
        };

        // Some response curve that makes no sense at all for now
        let set_rpm = (max_rpm - min_rpm) / temp as u16;
        thermal_service::execute_fan_request(fan::DeviceId(0), fan::Request::SetRpm(set_rpm))
            .await
            .unwrap();

        Ok(())
    }

    // This currently just polls temperature to check if above CRT and PROCHOT (interrupt alerts are reserved for THRS)
    // Might not actually be needed
    // TODO: Get more clarification on CRT and PROCHOT
    async fn crt_prochot_check(&self) {
        let state = self.state.lock().await;

        // If temp rises above PROCHOT, send the notification somewhere
        if state.cur_temp <= state.thresholds.1 || state.cur_temp >= state.thresholds.2 {
            thermal_service::send_service_msg(
                comms::EndpointID::External(comms::External::Host),
                &mptf::Notify::ProcHot,
            )
            .await;
        }

        // If temp rises above critical, notify Host and also notify power service to shutdown
        if state.cur_temp >= utils::dk_to_c(state.uid_table[uid::CRT_TEMP]) {
            thermal_service::send_service_msg(
                comms::EndpointID::External(comms::External::Host),
                &mptf::Notify::Critical,
            )
            .await;

            // TODO: Actually figure out message to send to Power service
            thermal_service::send_service_msg(
                comms::EndpointID::Internal(comms::Internal::Power),
                &mptf::Notify::Critical,
            )
            .await;
        }
    }

    async fn handle_fan_state(&self) {
        let mut state = self.state.lock().await;

        // Handle fan response to measured temperature
        match state.fan_state {
            FanState::Off => {
                // If temp rises above Fan Min On Temp, set fan to min RPM
                if state.cur_temp >= utils::dk_to_c(state.uid_table[uid::FAN_ON_TEMP]) {
                    let min_rpm =
                        match thermal_service::execute_fan_request(fan::DeviceId(0), fan::Request::GetMinRpm).await {
                            Ok(fan::Response::MinRpm(rpm)) => rpm,
                            _ => todo!(),
                        };

                    thermal_service::execute_fan_request(fan::DeviceId(0), fan::Request::SetRpm(min_rpm))
                        .await
                        .unwrap();
                    state.fan_state = FanState::On;
                    info!("\n\nFan turned ON\n\n");
                }
            }

            FanState::On => {
                // If temp rises above Fan Ramp Temp, set fan to begin ramp curve
                if state.cur_temp >= utils::dk_to_c(state.uid_table[uid::FAN_RAMP_TEMP]) {
                    state.fan_state = FanState::Ramping;
                    info!("\n\nFan ramping!\n\n");

                // If falls below on temp, turn fan off
                } else if state.cur_temp < utils::dk_to_c(state.uid_table[uid::FAN_ON_TEMP]) {
                    thermal_service::execute_fan_request(fan::DeviceId(0), fan::Request::SetRpm(0))
                        .await
                        .unwrap();
                    state.fan_state = FanState::Off;
                    info!("\n\nFan turned OFF\n\n");
                }
            }

            FanState::Ramping => {
                // If temp falls below ramp temp, set to On state
                if state.cur_temp < utils::dk_to_c(state.uid_table[uid::FAN_RAMP_TEMP]) {
                    let min_rpm =
                        match thermal_service::execute_fan_request(fan::DeviceId(0), fan::Request::GetMinRpm).await {
                            Ok(fan::Response::MinRpm(rpm)) => rpm,
                            _ => todo!(),
                        };

                    thermal_service::execute_fan_request(fan::DeviceId(0), fan::Request::SetRpm(min_rpm))
                        .await
                        .unwrap();
                    state.fan_state = FanState::On;
                }

                // If temp stays below max temp, continue ramp response
                if state.cur_temp < utils::dk_to_c(state.uid_table[uid::FAN_MAX_TEMP]) {
                    self.ramp_response(state.cur_temp).await.unwrap();

                // If above max, go to max state
                } else {
                    let max_rpm =
                        match thermal_service::execute_fan_request(fan::DeviceId(0), fan::Request::GetMaxRpm).await {
                            Ok(fan::Response::MaxRpm(rpm)) => rpm,
                            _ => todo!(),
                        };

                    thermal_service::execute_fan_request(fan::DeviceId(0), fan::Request::SetRpm(max_rpm))
                        .await
                        .unwrap();
                    state.fan_state = FanState::Max;
                    info!("\n\nFan at MAX!\n\n");
                }
            }

            FanState::Max => {
                if state.cur_temp < utils::dk_to_c(state.uid_table[uid::FAN_MAX_TEMP]) {
                    state.fan_state = FanState::Ramping;
                }
            }
        }
    }

    pub fn create() -> Self {
        Self {
            state: Mutex::new(State::default()),
        }
    }
}

impl mptf::Handler for DefaultHandler {
    async fn get_tmp(&self, _tzid: mptf::TzId) -> Result<mptf::Response, mptf::Error> {
        Ok(mptf::Response::GetTmp(utils::c_to_dk(self.state.lock().await.cur_temp)))
    }

    async fn get_thrs(&self, _tzid: mptf::TzId) -> Result<mptf::Response, mptf::Error> {
        let (timeout, low, high) = self.state.lock().await.thresholds;
        let low = utils::c_to_dk(low);
        let high = utils::c_to_dk(high);
        Ok(mptf::Response::GetThrs(timeout, low, high))
    }

    async fn set_thrs(
        &self,
        _tzid: mptf::TzId,
        timeout: mptf::Dword,
        low: mptf::DeciKelvin,
        high: mptf::DeciKelvin,
    ) -> Result<mptf::Response, mptf::Error> {
        let low = utils::dk_to_c(low);
        let high = utils::dk_to_c(high);
        self.state.lock().await.thresholds = (timeout, low, high);

        let _ = thermal_service::execute_sensor_request(sensor::DeviceId(0), sensor::Request::SetAlertLow(low)).await;
        let _ = thermal_service::execute_sensor_request(sensor::DeviceId(0), sensor::Request::SetAlertHigh(high)).await;

        Ok(mptf::Response::SetThrs)
    }

    async fn set_scp(
        &self,
        _tzid: mptf::TzId,
        mode: mptf::Dword,
        acoustic_lim: mptf::Dword,
        power_lim: mptf::Dword,
    ) -> Result<mptf::Response, mptf::Error> {
        let mut state = self.state.lock().await;
        state.cooling_policy = mode;
        state.acoustic_lim = acoustic_lim;
        state.power_lim = power_lim;

        Ok(mptf::Response::SetScp)
    }

    async fn get_var(&self, uid: mptf::Uid) -> Result<mptf::Response, mptf::Error> {
        self.state
            .lock()
            .await
            .uid_table
            .get(uid)
            .ok_or(mptf::Error::InvalidParameter)
            .map(|&val| mptf::Response::GetVar(val))
    }

    async fn set_var(&self, uid: mptf::Uid, val: mptf::Dword) -> Result<mptf::Response, mptf::Error> {
        let uid_table = &mut self.state.lock().await.uid_table;

        if let Some(inner_val) = uid_table.get_mut(uid) {
            *inner_val = val;
            Ok(mptf::Response::SetVar)
        } else {
            Err(mptf::Error::InvalidParameter)
        }
    }
}

/// Performs actual fan response algorithm
#[embassy_executor::task]
pub async fn task(spawner: embassy_executor::Spawner) {
    // Proof of concept logic
    // TODO: Improve

    let handler = DefaultHandler::create();

    spawner.must_spawn(threshold_task());

    loop {
        // Fetch current temperature from sensor
        handler.state.lock().await.cur_temp =
            match thermal_service::execute_sensor_request(sensor::DeviceId(0), sensor::Request::GetTemp).await {
                Ok(sensor::Response::Temp(temp)) => temp,
                Err(e) => {
                    error!("Error reading temperature: {:?}", e);
                    handler.state.lock().await.cur_temp
                }
                _ => {
                    error!("Unknown error occurred.");
                    handler.state.lock().await.cur_temp
                }
            };

        // Check if the current temperature exceeds CRT/PROCHOT and notify services if so
        handler.crt_prochot_check().await;

        // Handle fan state in response to current temperature
        handler.handle_fan_state().await;

        // Wait briefly
        Timer::after_millis(1000).await;
    }
}

/// Waits for sensor to trigger threshold alert, then notifies host
#[embassy_executor::task]
pub async fn threshold_task() {
    loop {
        match thermal_service::get_sensor(sensor::DeviceId(0))
            .await
            .unwrap()
            .wait_alert()
            .await
        {
            // TODO: Do we just send a singular Notify message Host, and Host then requests temp to determine which threshold was crossed?
            // Or should we separate into Notify::ThresholdLow and Notify::ThresholdHigh?
            sensor::Alert::ThresholdLow => todo!(),

            // Notify SoC/Host high threshold was tripped
            sensor::Alert::ThresholdHigh => {
                thermal_service::send_service_msg(
                    comms::EndpointID::External(comms::External::Host),
                    &mptf::Notify::Threshold,
                )
                .await
            }
        }
    }
}
