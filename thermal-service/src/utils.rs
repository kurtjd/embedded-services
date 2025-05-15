use crate::mptf;
use heapless::Deque;

// Sample buffer
pub(crate) struct SampleBuf<T: Default + Copy + core::fmt::Debug, const N: usize> {
    deque: Deque<T, N>,
}

impl<T: Default + Copy + core::fmt::Debug, const N: usize> SampleBuf<T, N> {
    pub(crate) fn new() -> Self {
        Self { deque: Deque::new() }
    }

    pub(crate) fn push(&mut self, sample: T) {
        if self.deque.is_full() {
            let _ = self.deque.pop_back();
        }

        self.deque.push_front(sample).expect("Will not panic");
    }

    pub(crate) fn recent(&self) -> T {
        *self.deque.front().unwrap_or(&T::default())
    }
}

/// Convert deciKelvin to degrees Celsius
pub const fn dk_to_c(dk: mptf::DeciKelvin) -> mptf::DegreesCelsius {
    (dk / 10) as mptf::DegreesCelsius - 273.15
}

/// Convert degrees Celsius to deciKelvin
pub const fn c_to_dk(c: mptf::DegreesCelsius) -> mptf::DeciKelvin {
    ((c + 273.15) * 10.0) as mptf::DeciKelvin
}
