//! Vendor adapters: the registry layer between device discovery
//! (`devices::scan`) and the protocol families (`protocol`).
//!
//! Layering rules (elaborated in docs/ADAPTATION.md):
//! - Wire frames are pure functions in `protocol`; adapters compose them and
//!   never build frames inline.
//! - One adapter is one vendor product line on one protocol family. OEM
//!   siblings that share a family reuse its protocol functions and add their
//!   own adapter with separate match rules; a VID set is never widened to
//!   cover another brand.
//! - Transport quirks (raw Windows handles, control-endpoint writes) stay
//!   inside the adapter's `read`; the trait never assumes a uniform transport.

mod atk;
mod atk_v2;
mod bigbigwon;
mod darkrim;
mod kzzi;

use hidapi::{DeviceInfo, HidApi};

pub use atk::AtkMouse;
pub use atk_v2::AtkV2Mouse;
pub use bigbigwon::BigbigwonGamepad;
pub use darkrim::DarkrimHeadset;
pub use kzzi::KzziKeyboard;

/// What a product line implements. Adapters declare capabilities so the UI can
/// decide what to offer; no adapter may declare a write capability before a
/// write path exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities(u16);

impl Capabilities {
    /// Battery level reads - the app's current feature.
    pub const BATTERY: Self = Self(1 << 0);
    /// Reserved: lighting control. Frames are recorded in probe research;
    /// stays undeclared until a write path lands.
    pub const LIGHTING: Self = Self(1 << 1);
    /// Reserved: DPI and key remapping.
    pub const CONFIGURATION: Self = Self(1 << 2);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// One adapter probe, applied to a `Reading` by `scan`.
#[derive(Debug, Default)]
pub struct Outcome {
    pub battery: Option<u8>,
    /// Replaces the USB product string when the protocol identifies the model.
    pub hardware_name: Option<String>,
    /// Protocol-reported per-unit identity; empty when the family has none.
    pub uid: String,
    /// Final UI status message.
    pub message: String,
}

/// One vendor product line.
pub trait VendorAdapter {
    /// USB match rule. Anchor on the VID plus usage page/usage; shared VIDs
    /// (e.g. 0x258A keyboards) must never match on the VID alone. Pin the PID
    /// only when the family owns it across firmware personalities.
    fn matches(&self, info: &DeviceInfo) -> bool;

    /// UI category: mouse, keyboard, gamepad or headset.
    fn kind(&self) -> &'static str;

    /// Features this family actually implements.
    fn capabilities(&self) -> Capabilities;

    /// Probe one receiver interface and report one outcome. Adapters own
    /// their transport and retries; errors surface as UI messages.
    fn read(&self, api: &HidApi, info: &DeviceInfo) -> Outcome;
}

/// Every supported product line, in scan order. Match rules are disjoint by
/// VID plus usage page, so the first matching adapter owns a device.
pub const ADAPTERS: &[&dyn VendorAdapter] = &[
    &AtkMouse,
    &AtkV2Mouse,
    &DarkrimHeadset,
    &KzziKeyboard,
    &BigbigwonGamepad,
];
