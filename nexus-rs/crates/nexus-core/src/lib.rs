//! The non-GUI parts of Nexus: settings, library, tags, audio engine and the online features.

pub mod audio;
pub mod config;
pub mod dsf;
pub mod inspect;
pub mod library;
pub mod lyrics;
pub mod paths;
pub mod playlists;
pub mod scanner;
pub mod secret;
pub mod skins;
pub mod util;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_NAME: &str = "Nexus";
