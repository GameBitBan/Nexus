//! WASAPI exclusive mode (Windows): Nexus takes the device, skipping the Windows mixer, so the output is
//! bit-perfect with EQ off and volume at 100 %. Filled in with the Windows build.

use super::Shared;
use std::sync::Arc;

pub(crate) fn open(_shared: Arc<Shared>, _rate: u32, _device: &str) -> Result<(Box<dyn std::any::Any>, u32), String> {
    Err("exclusive mode isn't built yet".into())
}
