use core::ops::Range;
use embassy_imxrt::pac;

pub(crate) fn nvram_read(address: usize) -> u32 {
    // TODO introduce RTC GPReg interface directly from embassy_imxrt

    // SAFETY: safe from single executor
    let rtc = unsafe { &*pac::Rtc::ptr() };

    rtc.gpreg(address).read().bits()
}

pub(crate) fn nvram_write(address: usize, value: u32) {
    // TODO introduce RTC GPReg interface directly from embassy_imxrt

    // SAFETY: safe from single executor
    let rtc = unsafe { &*pac::Rtc::ptr() };

    rtc.gpreg(address).write(|w|
        // SAFETY: safe from single executor 
        unsafe { w.bits(value) });
}

pub(crate) fn nvram_valid_range() -> Range<usize> {
    // TODO introduce RTC GPReg interface directly from embassy_imxrt
    // indices 0, 1, 2 are utilized by timer_driver in embassy_imxrt
    3..8
}
