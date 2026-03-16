//! Channel storage: model, persistence, and locking.

mod lock;
mod model;
mod storage;

pub use lock::{acquire_origin_lock, ChannelLock};
pub use model::{now_secs, session_key, ChannelRecord, ChannelStatus};
pub use storage::{
    delete_channel, find_reusable_channel, list_channels, load_channel, load_channels_by_origin,
    save_channel, take_channel_store_diagnostics, update_channel_close_state,
    ChannelStoreDiagnostics,
};
