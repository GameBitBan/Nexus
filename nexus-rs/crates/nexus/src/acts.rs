//! Things the UI asks for. Widgets push these while drawing; the app applies them after the frame
//! (so nothing changes under the widgets mid-draw).

use crate::app::Page;
use nexus_core::library::Track;
use std::sync::Arc;

pub type Tracks = Vec<Arc<Track>>;

pub enum Act {
    PlayList(Tracks, usize),
    PlayShuffled(Tracks),
    PlayQueueIndex(usize),
    QueueNext(Tracks),
    QueueAdd(Tracks),
    QueueRemove(Vec<usize>),
    QueueClear,
    PlaylistAdd(String, Tracks),
    PlaylistNew(Tracks),
    PlaylistRemoveRows(String, Vec<usize>),
    PlaylistRename(String),
    PlaylistDelete(String),
    TogglePlay,
    Next,
    Prev,
    Seek(f64),
    SeekFrac(f32),
    ToggleShuffle,
    CycleRepeat,
    ToggleMute,
    SetVolume(f64),
    TogglePanel,
    ToggleRemaining,
    Go(Page),
    Back,
    OpenSettings,
    ToggleLyrics,
    ToggleVisualizer,
    OpenEq,
    Rescan(bool),
    AddFolder,
    OpenFolder(String),
    CopyText(String),
    GotoPlayingAlbum,
    GotoPlayingArtist,
    SwitchLayout(String),
    ToggleMini,
    TogglePin,
    Toast(String),
}
