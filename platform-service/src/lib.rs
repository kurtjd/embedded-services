#![no_std]

/// NVRAM platform service abstraction
pub mod nvram;

#[cfg(any(feature = "imxrt", feature = "imxrt685"))]
pub(crate) mod imxrt;

#[cfg(any(feature = "imxrt", feature = "imxrt685"))]
pub(crate) use imxrt::*;

#[cfg(not(any(feature = "imxrt", feature = "imxrt685")))]
mod defaults {
    use core::ops::Range;

    pub(crate) fn nvram_read(_address: usize) -> u32 {
        0
    }
    pub(crate) fn nvram_write(_address: usize, _value: u32) {}

    pub(crate) fn nvram_valid_range() -> Range<usize> {
        0..0
    }
}

#[cfg(not(any(feature = "imxrt", feature = "imxrt685")))]
pub(crate) use defaults::*;

#[cfg(test)]
mod tests {
    use super::*;
}
