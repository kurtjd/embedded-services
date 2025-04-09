//! Event definitions
use bitfield::bitfield;
use bitvec::BitArr;
use embedded_usb_pd::GlobalPortId;

bitfield! {
    /// Raw bitfield of possible port events
    #[derive(Copy, Clone, PartialEq, Eq)]
    #[cfg_attr(feature = "defmt", derive(defmt::Format))]
    struct PortEventKindRaw(u32);
    impl Debug;
    /// Plug inserted or removed
    pub u8, plug_inserted_or_removed, set_plug_inserted_or_removed: 0, 0;
    /// New power contract as provider
    pub u8, new_power_contract_as_provider, set_new_power_contract_as_provider: 2, 2;
    /// New power contract as consumer
    pub u8, new_power_contract_as_consumer, set_new_power_contract_as_consumer: 3, 3;
}

/// Type-safe wrapper for the raw port event kind
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct PortEventKind(PortEventKindRaw);

impl PortEventKind {
    /// Create a new PortEventKind with no pending events
    pub const fn none() -> Self {
        Self(PortEventKindRaw(0))
    }

    /// Returns the union of self and other
    pub fn union(self, other: PortEventKind) -> PortEventKind {
        // This spacing is what rustfmt wants
        PortEventKind(PortEventKindRaw(self.0 .0 | other.0 .0))
    }

    /// Returns true if a plug was inserted or removed
    pub fn plug_inserted_or_removed(self) -> bool {
        self.0.plug_inserted_or_removed() != 0
    }

    /// Sets the plug inserted or removed event
    pub fn set_plug_inserted_or_removed(&mut self, value: bool) {
        self.0.set_plug_inserted_or_removed(value.into());
    }

    /// Returns true if a new power contract was established as provider
    pub fn new_power_contract_as_provider(&self) -> bool {
        self.0.new_power_contract_as_provider() != 0
    }

    /// Sets the new power contract as provider event
    pub fn set_new_power_contract_as_provider(&mut self, value: bool) {
        self.0.set_new_power_contract_as_provider(value.into());
    }

    /// Returns true if a new power contract was established as consumer
    pub fn new_power_contract_as_consumer(self) -> bool {
        self.0.new_power_contract_as_consumer() != 0
    }

    /// Sets the new power contract as consumer event
    pub fn set_new_power_contract_as_consumer(&mut self, value: bool) {
        self.0.set_new_power_contract_as_consumer(value.into());
    }
}

/// Bit vector type to store pending port events
type PortEventFlagsVec = BitArr!(for 32, in u32);

/// Pending port events
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct PortEventFlags(PortEventFlagsVec);

impl PortEventFlags {
    /// Creates a new PortEventFlags with no pending events
    pub const fn none() -> Self {
        Self(PortEventFlagsVec::ZERO)
    }

    /// Returns true if there are no pending events
    pub fn is_none(&self) -> bool {
        self.0 == PortEventFlagsVec::ZERO
    }

    /// Marks the given port as pending
    pub fn pend_port(&mut self, port: GlobalPortId) {
        self.0.set(port.0 as usize, true);
    }

    /// Returns true if the given port is pending
    pub fn is_pending(&self, port: GlobalPortId) -> bool {
        self.0[port.0 as usize]
    }

    /// Returns a combination of the current event flags and other
    pub fn union(&self, other: PortEventFlags) -> PortEventFlags {
        PortEventFlags(self.0 | other.0)
    }

    /// Returns the number of bits in the event
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl From<PortEventFlags> for u32 {
    fn from(flags: PortEventFlags) -> Self {
        flags.0.data[0]
    }
}
