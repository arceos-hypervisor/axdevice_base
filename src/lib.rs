//! Basic traits and structures for emulated devices in ArceOS hypervisor.
//!
//! This crate contains:
//! - [`BaseDeviceOps`] trait: The trait that all emulated devices must implement.
//! - [`EmuDeviceType`] enum: Enumeration representing the type of emulator devices.
//!   (Already moved to `axvmconfig` crate.)
//! - [`EmulatedDeviceConfig`]: Configuration structure for device initialization.
//! - Multi-region address support types: [`RegionId`], [`RegionHit`], [`DeviceRegion`], [`RegionDescriptor`]

#![no_std]
#![feature(trait_alias)]
// trait_upcasting has been stabilized in Rust 1.86, but we still need a while to update the minimum
// Rust version of Axvisor.
#![allow(stable_features)]
#![feature(trait_upcasting)]
#![allow(incomplete_features)]
#![feature(generic_const_exprs)]

extern crate alloc;

use alloc::{string::String, sync::Arc, vec::Vec};
use arrayvec::ArrayVec;
use core::any::Any;

use axaddrspace::{
    GuestPhysAddrRange,
    device::{AccessWidth, DeviceAddrRange, PortRange, SysRegAddrRange},
};
use axerrno::AxResult;

pub use axvmconfig::EmulatedDeviceType as EmuDeviceType;

// ============================================================================
// Interrupt Management Types
// ============================================================================

/// Interrupt trigger mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerMode {
    /// Edge-triggered interrupt.
    Edge,
    /// Level-triggered interrupt.
    Level,
    /// Message Signaled Interrupt.
    Msi,
    /// Extended Message Signaled Interrupt.
    MsiX,
}

/// CPU affinity strategy for interrupt routing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CpuAffinity {
    /// Always route to a fixed CPU.
    Fixed(usize),
    /// Round-robin across CPUs.
    RoundRobin,
    /// Load balance based on queue length.
    LoadBalance,
    /// Broadcast to all CPUs.
    Broadcast,
}

/// Interrupt type (primary or additional).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqType {
    /// Primary interrupt.
    Primary,
    /// Additional interrupt with index.
    Additional(u32),
}

/// Interrupt configuration for a device.
#[derive(Debug, Clone)]
pub struct InterruptConfig {
    /// Primary IRQ number.
    pub primary_irq: u32,
    /// Additional IRQ numbers for multi-interrupt devices.
    pub additional_irqs: Vec<u32>,
    /// Trigger mode for the interrupt.
    pub trigger_mode: TriggerMode,
    /// CPU affinity strategy.
    pub cpu_affinity: CpuAffinity,
    /// Interrupt priority (0-255, higher is more important).
    pub priority: u8,
}

/// Interrupt trigger trait for devices.
///
/// This trait is implemented by the interrupt management system and injected
/// into devices during registration. Devices use this trait to request interrupts
/// without directly depending on architecture-specific interrupt controllers.
///
/// **Deprecated**: Use [`DeviceNotifier`] instead for new devices.
pub trait InterruptTrigger: Send + Sync {
    /// Trigger an interrupt.
    ///
    /// # Arguments
    ///
    /// * `irq_type` - The type of interrupt to trigger (Primary or Additional).
    fn trigger(&self, irq_type: IrqType) -> AxResult;

    /// Clear an interrupt (for level-triggered interrupts).
    ///
    /// # Arguments
    ///
    /// * `irq_type` - The type of interrupt to clear.
    fn clear(&self, irq_type: IrqType) -> AxResult;
}

// ============================================================================
// Notification Types (New API)
// ============================================================================

/// Notification method for devices.
///
/// Devices can choose different notification methods based on their requirements:
/// - High-frequency devices may prefer `Poll` to avoid interrupt overhead
/// - Low-frequency devices may prefer `Interrupt` for power efficiency
/// - Test scenarios may prefer `Callback` for synchronous verification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NotifyMethod {
    /// Hardware interrupt - traditional method via vPLIC/vGIC injection.
    /// Best for: low-frequency devices, power-sensitive scenarios.
    #[default]
    Interrupt,
    /// Polling flag - device sets flag, vCPU loop checks periodically.
    /// Best for: high-frequency devices, low-latency scenarios.
    Poll,
    /// Synchronous callback - immediate execution (beware of deadlocks).
    /// Best for: testing, simple scenarios.
    Callback,
    /// Event queue - asynchronous events supporting batch processing.
    /// Best for: devices with burst traffic patterns.
    Event,
}

/// Device event types for notification.
///
/// These semantic event types replace the generic `IrqType` for better clarity
/// about what happened in the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceEvent {
    /// Data is ready to be read (e.g., VirtIO receiveq has data).
    DataReady,
    /// Space is available for writing (e.g., VirtIO transmitq has room).
    SpaceAvailable,
    /// Device configuration has changed.
    ConfigChanged,
    /// Generic interrupt request (for backward compatibility with IrqType).
    Irq(IrqType),
    /// Custom device-specific event.
    Custom(u32),
}

impl DeviceEvent {
    /// Convert event to a poll flag bit.
    ///
    /// Each event type maps to a unique bit in a 32-bit flag word,
    /// allowing multiple events to be OR'd together.
    pub const fn as_flag(&self) -> u32 {
        match self {
            DeviceEvent::DataReady => 1 << 0,
            DeviceEvent::SpaceAvailable => 1 << 1,
            DeviceEvent::ConfigChanged => 1 << 2,
            DeviceEvent::Irq(IrqType::Primary) => 1 << 3,
            DeviceEvent::Irq(IrqType::Additional(n)) => 1 << (4 + (*n % 12)),
            DeviceEvent::Custom(n) => 1 << (16 + (*n % 16)),
        }
    }

    /// Check if a flag word contains this event.
    pub const fn is_set_in(&self, flags: u32) -> bool {
        (flags & self.as_flag()) != 0
    }
}

/// Notification configuration for a device.
///
/// This is the new configuration structure that replaces `InterruptConfig`,
/// supporting multiple notification methods beyond just interrupts.
#[derive(Debug, Clone)]
pub struct NotificationConfig {
    /// Notification method.
    pub method: NotifyMethod,
    /// Primary IRQ number (required for `Interrupt` method).
    pub primary_irq: Option<u32>,
    /// Additional IRQ numbers for multi-interrupt devices.
    pub additional_irqs: Vec<u32>,
    /// Trigger mode for interrupts.
    pub trigger_mode: TriggerMode,
    /// CPU affinity strategy.
    pub cpu_affinity: CpuAffinity,
    /// Priority (0-255, higher is more important).
    pub priority: u8,
    /// Enable event coalescing to reduce notification frequency.
    pub coalesce: bool,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            method: NotifyMethod::Interrupt,
            primary_irq: None,
            additional_irqs: Vec::new(),
            trigger_mode: TriggerMode::Level,
            cpu_affinity: CpuAffinity::Fixed(0),
            priority: 100,
            coalesce: false,
        }
    }
}

impl NotificationConfig {
    /// Create an interrupt-based notification configuration.
    ///
    /// This is the most common configuration for devices that use traditional
    /// interrupt-based notification.
    pub fn interrupt(irq: u32) -> Self {
        Self {
            method: NotifyMethod::Interrupt,
            primary_irq: Some(irq),
            ..Default::default()
        }
    }

    /// Create a poll-based notification configuration.
    ///
    /// Use this for high-frequency devices where interrupt overhead is too high.
    pub fn poll() -> Self {
        Self {
            method: NotifyMethod::Poll,
            primary_irq: None,
            coalesce: true,
            ..Default::default()
        }
    }

    /// Create an event-based notification configuration.
    pub fn event() -> Self {
        Self {
            method: NotifyMethod::Event,
            primary_irq: None,
            coalesce: true,
            ..Default::default()
        }
    }

    /// Convert from legacy `InterruptConfig`.
    pub fn from_interrupt_config(config: &InterruptConfig) -> Self {
        Self {
            method: NotifyMethod::Interrupt,
            primary_irq: Some(config.primary_irq),
            additional_irqs: config.additional_irqs.clone(),
            trigger_mode: config.trigger_mode,
            cpu_affinity: config.cpu_affinity.clone(),
            priority: config.priority,
            coalesce: false,
        }
    }

    /// Convert to legacy `InterruptConfig` (for backward compatibility).
    ///
    /// Returns `None` if this config doesn't use interrupt method or has no IRQ.
    pub fn to_interrupt_config(&self) -> Option<InterruptConfig> {
        if self.method != NotifyMethod::Interrupt {
            return None;
        }
        Some(InterruptConfig {
            primary_irq: self.primary_irq?,
            additional_irqs: self.additional_irqs.clone(),
            trigger_mode: self.trigger_mode,
            cpu_affinity: self.cpu_affinity.clone(),
            priority: self.priority,
        })
    }

    /// Builder: set CPU affinity.
    pub fn with_cpu_affinity(mut self, affinity: CpuAffinity) -> Self {
        self.cpu_affinity = affinity;
        self
    }

    /// Builder: set priority.
    pub fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }

    /// Builder: enable/disable coalescing.
    pub fn with_coalesce(mut self, coalesce: bool) -> Self {
        self.coalesce = coalesce;
        self
    }

    /// Builder: set trigger mode.
    pub fn with_trigger_mode(mut self, mode: TriggerMode) -> Self {
        self.trigger_mode = mode;
        self
    }
}

/// Device notifier trait for sending notifications to guests.
///
/// This is the new unified notification API that replaces `InterruptTrigger`.
/// It supports multiple notification methods (interrupt, poll, callback, event)
/// and provides semantic event types for better clarity.
///
/// # Example
///
/// ```rust,ignore
/// // In device implementation
/// fn handle_write(&self, addr: GuestPhysAddr, width: AccessWidth, val: usize) -> AxResult {
///     // ... device logic ...
///
///     if has_data_for_guest {
///         if let Some(notifier) = self.notifier.read().as_ref() {
///             notifier.notify(DeviceEvent::DataReady)?;
///         }
///     }
///     Ok(())
/// }
/// ```
pub trait DeviceNotifier: Send + Sync {
    /// Send a notification to the guest.
    ///
    /// The notification will be delivered according to the device's configured
    /// notification method (interrupt, poll flag, callback, or event queue).
    fn notify(&self, event: DeviceEvent) -> AxResult;

    /// Clear a notification (for level-triggered interrupts).
    ///
    /// This is typically called when the guest acknowledges the interrupt.
    fn clear(&self, _event: DeviceEvent) -> AxResult {
        Ok(()) // Default: no-op
    }

    /// Get the notification method for this notifier.
    fn method(&self) -> NotifyMethod;

    /// Check if there are pending notifications (for Poll method).
    ///
    /// Returns `true` if the device has set poll flags that haven't been cleared.
    fn has_pending(&self) -> bool {
        false
    }
}

/// Adapter to use `DeviceNotifier` as `InterruptTrigger` (backward compatibility).
impl<T: DeviceNotifier + ?Sized> InterruptTrigger for T {
    fn trigger(&self, irq_type: IrqType) -> AxResult {
        self.notify(DeviceEvent::Irq(irq_type))
    }

    fn clear(&self, irq_type: IrqType) -> AxResult {
        DeviceNotifier::clear(self, DeviceEvent::Irq(irq_type))
    }
}

// ============================================================================
// Multi-Region Address Support Types
// ============================================================================

/// Region ID for fast lookup results (avoids string comparison).
///
/// Pre-defined constants are provided for common region types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RegionId(pub u8);

impl RegionId {
    /// Control region (e.g., VirtIO control registers).
    pub const CONTROL: Self = Self(0);
    /// Status region (e.g., ISR registers).
    pub const STATUS: Self = Self(1);
    /// Notification region (e.g., VirtIO doorbell).
    pub const NOTIFICATION: Self = Self(2);
    /// Configuration region (e.g., device config space).
    pub const CONFIG: Self = Self(3);
    /// Data region (e.g., data buffers).
    pub const DATA: Self = Self(4);
    /// Default region for single-region devices.
    pub const DEFAULT: Self = Self(5);
    // PCI BARs (16-21)
    /// PCI BAR0.
    pub const BAR0: Self = Self(16);
    /// PCI BAR1.
    pub const BAR1: Self = Self(17);
    /// PCI BAR2.
    pub const BAR2: Self = Self(18);
    /// PCI BAR3.
    pub const BAR3: Self = Self(19);
    /// PCI BAR4.
    pub const BAR4: Self = Self(20);
    /// PCI BAR5.
    pub const BAR5: Self = Self(21);
}

/// Region lookup result (zero-allocation, returned on stack).
///
/// This structure is returned by `region_lookup()` and contains all information
/// needed to handle an access to a specific region.
#[derive(Clone, Copy, Debug)]
pub struct RegionHit {
    /// Region ID.
    pub region_id: RegionId,
    /// Offset relative to region base.
    pub offset: usize,
    /// Region type.
    pub region_type: RegionType,
    /// Access permissions.
    pub permissions: Permissions,
}

/// Region type classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RegionType {
    /// Generic register region.
    #[default]
    Generic,
    /// Control registers.
    Control,
    /// Status registers (typically read-only).
    Status,
    /// Data buffer region.
    Data,
    /// Notification/doorbell region (VirtIO).
    Notification,
    /// Device configuration space.
    Config,
    /// PCI configuration space.
    PciConfig,
    /// PCI BAR region.
    PciBar(u8),
}

/// Access permissions for a region.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Permissions {
    /// Read and write allowed.
    #[default]
    ReadWrite,
    /// Read only.
    ReadOnly,
    /// Write only.
    WriteOnly,
    /// No access allowed.
    None,
}

/// Device address region descriptor.
///
/// Describes a single address region of a device, including its ID, name,
/// base address, size, type, and access permissions.
#[derive(Clone, Debug)]
pub struct DeviceRegion {
    /// Region ID for fast matching.
    pub id: RegionId,
    /// Region name (for debugging).
    pub name: &'static str,
    /// Base address.
    pub base: usize,
    /// Size in bytes.
    pub size: usize,
    /// Region type.
    pub region_type: RegionType,
    /// Access permissions.
    pub permissions: Permissions,
}

impl DeviceRegion {
    /// Create a new device region.
    pub const fn new(id: RegionId, name: &'static str, base: usize, size: usize) -> Self {
        Self {
            id,
            name,
            base,
            size,
            region_type: RegionType::Generic,
            permissions: Permissions::ReadWrite,
        }
    }

    /// Set the region type.
    pub const fn with_type(mut self, region_type: RegionType) -> Self {
        self.region_type = region_type;
        self
    }

    /// Set the access permissions.
    pub const fn with_permissions(mut self, permissions: Permissions) -> Self {
        self.permissions = permissions;
        self
    }

    /// Check if the address falls within this region.
    #[inline]
    pub const fn contains(&self, addr: usize) -> bool {
        addr >= self.base && addr < self.base + self.size
    }

    /// Get the end address (exclusive).
    #[inline]
    pub const fn end(&self) -> usize {
        self.base + self.size
    }

    /// Try to match an address, returning hit info if successful (zero-allocation).
    #[inline]
    pub fn try_hit(&self, addr: usize) -> Option<RegionHit> {
        if self.contains(addr) {
            Some(RegionHit {
                region_id: self.id,
                offset: addr - self.base,
                region_type: self.region_type,
                permissions: self.permissions,
            })
        } else {
            None
        }
    }
}

/// Maximum number of regions per device.
pub const MAX_REGIONS_PER_DEVICE: usize = 8;

/// Static region descriptor (compile-time sized, no heap allocation).
///
/// Uses `ArrayVec` to avoid `Vec` heap allocations, supporting up to
/// [`MAX_REGIONS_PER_DEVICE`] regions per device.
#[derive(Clone, Debug)]
pub struct RegionDescriptor {
    regions: ArrayVec<DeviceRegion, MAX_REGIONS_PER_DEVICE>,
}

impl Default for RegionDescriptor {
    fn default() -> Self {
        Self::new()
    }
}

impl RegionDescriptor {
    /// Create an empty region descriptor.
    pub fn new() -> Self {
        Self {
            regions: ArrayVec::new(),
        }
    }

    /// Add a region to the descriptor (builder pattern).
    pub fn with_region(mut self, region: DeviceRegion) -> Self {
        self.regions.push(region);
        self
    }

    /// Create a single-region descriptor.
    pub fn single(base: usize, size: usize) -> Self {
        Self::new().with_region(DeviceRegion::new(RegionId::DEFAULT, "default", base, size))
    }

    /// Get all regions.
    pub fn regions(&self) -> &[DeviceRegion] {
        &self.regions
    }

    /// Get the number of regions.
    pub fn len(&self) -> usize {
        self.regions.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    /// Lookup an address in all regions (O(n), but n ≤ 8).
    #[inline]
    pub fn lookup(&self, addr: usize) -> Option<RegionHit> {
        self.regions.iter().find_map(|r| r.try_hit(addr))
    }
}


/// Represents the configuration of an emulated device for a virtual machine.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct EmulatedDeviceConfig {
    /// The name of the device
    pub name: String,
    /// The base IPA (Intermediate Physical Address) of the device.
    pub base_ipa: usize,
    /// The length of the device.
    pub length: usize,
    /// The IRQ (Interrupt Request) ID of the device.
    pub irq_id: usize,
    /// The type of emulated device.
    pub emu_type: usize,
    /// The config_list of the device
    pub cfg_list: Vec<usize>,
}

/// [`BaseDeviceOps`] is the trait that all emulated devices must implement.
///
/// # Thread Safety
///
/// Devices must implement `Send + Sync` to enable safe concurrent access across
/// multiple vCPUs. The device framework provides per-device locks to ensure
/// exclusive access during operations.
///
/// # Notification Support
///
/// Devices that need to notify guests should:
/// 1. Implement `notification_config()` to declare their notification requirements
/// 2. Implement `set_notifier()` to receive the notifier from the framework
/// 3. Use the notifier to send notifications via `notifier.notify(DeviceEvent::DataReady)`
///
/// For backward compatibility, the old `interrupt_config()` and `set_interrupt_trigger()`
/// methods are still supported but deprecated.
///
/// # Multi-Region Address Support
///
/// Devices with multiple address regions (e.g., VirtIO, PCI) should:
/// 1. Implement `region_descriptor()` to declare their regions (called once at registration)
/// 2. Optionally override `region_lookup()` to provide a faster inline implementation
/// 3. Use `RegionHit` in `handle_read/write` for zero-allocation region dispatch
pub trait BaseDeviceOps<R: DeviceAddrRange>: Any + Send + Sync {
    /// Returns the type of the emulated device.
    fn emu_type(&self) -> EmuDeviceType;

    /// Returns all address ranges of the emulated device.
    ///
    /// Most devices have a single contiguous address range, but some devices
    /// (e.g., PCI devices, multi-core interrupt controllers) may have multiple
    /// non-contiguous ranges.
    ///
    /// This method must return at least one address range.
    fn address_ranges(&self) -> &[R];

    /// Returns the primary address range of the emulated device.
    ///
    /// **Deprecated**: This method is provided for backward compatibility with older devices.
    /// New code should use `address_ranges()` instead.
    ///
    /// Default implementation returns a copy of the first range from `address_ranges()`.
    ///
    /// # Panics
    ///
    /// Panics if `address_ranges()` returns an empty slice.
    #[deprecated(since = "0.2.0", note = "Use address_ranges() instead")]
    fn address_range(&self) -> R
    where
        R: Copy,
    {
        self.address_ranges()[0]
    }

    /// Handles a read operation on the emulated device.
    fn handle_read(&self, addr: R::Addr, width: AccessWidth) -> AxResult<usize>;

    /// Handles a write operation on the emulated device.
    fn handle_write(&self, addr: R::Addr, width: AccessWidth, val: usize) -> AxResult;

    // ========================================================================
    // Notification Methods (New API)
    // ========================================================================

    /// Returns the notification configuration for this device.
    ///
    /// Devices that need to notify guests should override this method to return
    /// their notification configuration. The framework will use this to set up
    /// the appropriate notification mechanism.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn notification_config(&self) -> Option<NotificationConfig> {
    ///     Some(NotificationConfig::interrupt(self.irq_id)
    ///         .with_priority(100)
    ///         .with_coalesce(true))
    /// }
    /// ```
    fn notification_config(&self) -> Option<NotificationConfig> {
        // Default: try to convert from legacy interrupt_config for backward compatibility
        #[allow(deprecated)]
        self.interrupt_config().map(|ic| NotificationConfig::from_interrupt_config(&ic))
    }

    /// Sets the notifier for this device.
    ///
    /// This method is called by the device framework during device registration
    /// if the device declares notification support via `notification_config()`.
    ///
    /// Devices should store the notifier and use it to send notifications:
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn set_notifier(&self, notifier: Arc<dyn DeviceNotifier>) {
    ///     *self.notifier.write() = Some(notifier);
    /// }
    ///
    /// fn handle_write(&self, addr: GuestPhysAddr, width: AccessWidth, val: usize) -> AxResult {
    ///     // ... device logic ...
    ///
    ///     if has_data_for_guest {
    ///         if let Some(notifier) = self.notifier.read().as_ref() {
    ///             notifier.notify(DeviceEvent::DataReady)?;
    ///         }
    ///     }
    ///     Ok(())
    /// }
    /// ```
    fn set_notifier(&self, _notifier: Arc<dyn DeviceNotifier>) {
        // Default implementation does nothing. Devices with notification support
        // must override this to store the notifier.
    }

    // ========================================================================
    // Legacy Interrupt Methods (Deprecated)
    // ========================================================================

    /// Returns the interrupt configuration for this device.
    ///
    /// **Deprecated**: Use `notification_config()` instead.
    #[deprecated(since = "0.3.0", note = "Use notification_config() instead")]
    fn interrupt_config(&self) -> Option<InterruptConfig> {
        None
    }

    /// Sets the interrupt trigger for this device.
    ///
    /// **Deprecated**: Use `set_notifier()` instead.
    ///
    /// Note: This method does not automatically forward to `set_notifier()` because
    /// trait object casting is not supported. Devices should migrate to using
    /// `set_notifier()` for new code.
    #[deprecated(since = "0.3.0", note = "Use set_notifier() instead")]
    fn set_interrupt_trigger(&self, _trigger: Arc<dyn InterruptTrigger>) {
        // Default implementation does nothing.
        // Devices using the old API should override this method.
        // Devices using the new API should override set_notifier() instead.
    }

    // ========================================================================
    // Multi-Region Address Support Methods
    // ========================================================================

    /// Returns the region descriptor for this device (called once at registration).
    ///
    /// Devices with multiple address regions should override this method.
    /// The framework will cache the result and use it for address routing.
    ///
    /// Returns `None` for single-region devices (uses `address_ranges()` instead).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn region_descriptor(&self) -> Option<RegionDescriptor> {
    ///     Some(RegionDescriptor::new()
    ///         .with_region(DeviceRegion::new(
    ///             RegionId::CONTROL, "control",
    ///             self.base, 0x100
    ///         ).with_type(RegionType::Control))
    ///         .with_region(DeviceRegion::new(
    ///             RegionId::NOTIFICATION, "notify",
    ///             self.base + 0x1000, 0x1000
    ///         ).with_type(RegionType::Notification)
    ///          .with_permissions(Permissions::WriteOnly)))
    /// }
    /// ```
    fn region_descriptor(&self) -> Option<RegionDescriptor> {
        None
    }

    /// Fast region lookup for hot path (zero-allocation).
    ///
    /// Devices can override this method to provide a more efficient inline
    /// implementation. The default uses `region_descriptor().lookup()`.
    ///
    /// # Performance
    ///
    /// For devices with fixed layouts (e.g., VirtIO MMIO), implementing this
    /// method with `#[inline(always)]` and direct offset calculations can
    /// reduce lookup time from ~30ns to ~5ns.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// #[inline(always)]
    /// fn region_lookup(&self, addr: usize) -> Option<RegionHit> {
    ///     let offset = addr.checked_sub(self.base)?;
    ///     if offset < 0x100 {
    ///         Some(RegionHit {
    ///             region_id: RegionId::CONTROL,
    ///             offset,
    ///             region_type: RegionType::Control,
    ///             permissions: Permissions::ReadWrite,
    ///         })
    ///     } else {
    ///         None
    ///     }
    /// }
    /// ```
    #[inline]
    fn region_lookup(&self, addr: usize) -> Option<RegionHit> {
        self.region_descriptor()?.lookup(addr)
    }

    /// Notify the framework that regions have changed (for PCI BAR remapping).
    ///
    /// Call this after modifying BAR addresses. The framework will re-read
    /// `region_descriptor()` and update its internal cache.
    ///
    /// Returns `true` if dynamic region changes are supported.
    fn notify_region_change(&self) -> bool {
        false
    }
}

/// Determines whether the given device is of type `T` and calls the provided function `f` with a
/// reference to the device if it is.
pub fn map_device_of_type<T: BaseDeviceOps<R>, R: DeviceAddrRange, U, F: FnOnce(&T) -> U>(
    device: &Arc<dyn BaseDeviceOps<R>>,
    f: F,
) -> Option<U> {
    let any_arc: Arc<dyn Any> = device.clone();

    any_arc.downcast_ref::<T>().map(f)
}

// trait aliases are limited yet: https://github.com/rust-lang/rfcs/pull/3437
/// [`BaseMmioDeviceOps`] is the trait that all emulated MMIO devices must implement.
/// It is a trait alias of [`BaseDeviceOps`] with [`GuestPhysAddrRange`] as the address range.
pub trait BaseMmioDeviceOps = BaseDeviceOps<GuestPhysAddrRange>;
/// [`BaseSysRegDeviceOps`] is the trait that all emulated system register devices must implement.
/// It is a trait alias of [`BaseDeviceOps`] with [`SysRegAddrRange`] as the address range.
pub trait BaseSysRegDeviceOps = BaseDeviceOps<SysRegAddrRange>;
/// [`BasePortDeviceOps`] is the trait that all emulated port devices must implement.
/// It is a trait alias of [`BaseDeviceOps`] with [`PortRange`] as the address range.
pub trait BasePortDeviceOps = BaseDeviceOps<PortRange>;

#[cfg(test)]
mod test;
