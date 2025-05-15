//! Definitions for standard MPTF messages the generic Thermal service can expect
//! These will need to be fleshed out and ensure they meet required MPTF/ACPI specs and whatnot
//!
//! Transport services such as eSPI and SSH would then need to make sure they agree on this format
//! as they would then be responsible for converting raw data into these types and forwarding them
//! to thermal.
pub type TzId = u8;
pub type Dword = u32;
pub type DeciKelvin = Dword;
pub type DegreesCelsius = f32;

// Should we expect a str or just u128? Use str for now.
pub type Uid = &'static str;

#[derive(Debug, Clone, Copy)]
pub enum Error {
    InvalidParameter,
    UnsupportedRevision,
    HardwareError,
}

impl From<Error> for u8 {
    fn from(value: Error) -> Self {
        match value {
            Error::InvalidParameter => 1,
            Error::UnsupportedRevision => 2,
            Error::HardwareError => 3,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Request {
    // EC_THM_GET_TMP
    GetTmp(TzId),
    // EC_THM_GET_THRS
    GetThrs(TzId),
    // EC_THM_SET_THRS
    SetThrs(TzId, Dword, DeciKelvin, DeciKelvin),
    // EC_THM_SET_SCP
    SetScp(TzId, Dword, Dword, Dword),
    // EC_THM_GET_VAR
    GetVar(&'static str),
    // EC_THM_SET_VAR
    SetVar(&'static str, Dword),
}

#[derive(Debug, Clone, Copy)]
pub enum Response {
    // Standard command
    GetTmp(DeciKelvin),
    GetThrs(Dword, DeciKelvin, DeciKelvin),
    SetThrs,
    SetScp,
    GetVar(Dword),
    SetVar,
}

#[derive(Debug, Clone, Copy)]
pub enum Notify {
    Threshold,
    Critical,
    ProcHot,
}

// TODO: Remove this `allow`
#[allow(async_fn_in_trait)]
pub trait Handler {
    async fn get_tmp(&self, tzid: TzId) -> Result<Response, Error>;
    async fn get_thrs(&self, tzid: TzId) -> Result<Response, Error>;
    async fn set_thrs(&self, tzid: TzId, timeout: Dword, low: DeciKelvin, high: DeciKelvin) -> Result<Response, Error>;
    async fn set_scp(&self, tzid: TzId, mode: Dword, acoustic_lim: Dword, power_lim: Dword) -> Result<Response, Error>;
    async fn get_var(&self, uid: Uid) -> Result<Response, Error>;
    async fn set_var(&self, uid: Uid, val: Dword) -> Result<Response, Error>;
}

impl<T: Handler + ?Sized> Handler for &T {
    #[inline]
    async fn get_tmp(&self, tzid: TzId) -> Result<Response, Error> {
        T::get_tmp(self, tzid).await
    }

    #[inline]
    async fn get_thrs(&self, tzid: TzId) -> Result<Response, Error> {
        T::get_thrs(self, tzid).await
    }

    #[inline]
    async fn set_thrs(&self, tzid: TzId, timeout: Dword, low: DeciKelvin, high: DeciKelvin) -> Result<Response, Error> {
        T::set_thrs(self, tzid, timeout, low, high).await
    }

    #[inline]
    async fn set_scp(&self, tzid: TzId, mode: Dword, acoustic_lim: Dword, power_lim: Dword) -> Result<Response, Error> {
        T::set_scp(self, tzid, mode, acoustic_lim, power_lim).await
    }

    #[inline]
    async fn set_var(&self, uid: Uid, val: Dword) -> Result<Response, Error> {
        T::set_var(self, uid, val).await
    }

    #[inline]
    async fn get_var(&self, uid: Uid) -> Result<Response, Error> {
        T::get_var(self, uid).await
    }
}
