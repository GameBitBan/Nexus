//! The non-GUI parts of Nexus: settings, library, tags, audio engine and the online features.

pub mod audio;
pub mod config;
pub mod discord;
pub mod discover;
pub mod dl;
pub mod dsf;
pub mod importer;
pub mod imports;
pub mod inspect;
pub mod library;
pub mod lyrics;
pub mod net;
pub mod netcheck;
pub mod paths;
pub mod playlists;
pub mod scanner;
pub mod selftest;
pub mod secret;
pub mod skins;
pub mod slsk;
pub mod tags;
pub mod update;
pub mod util;
pub mod webmusic;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_NAME: &str = "Nexus";
