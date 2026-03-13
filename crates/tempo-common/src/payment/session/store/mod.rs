//! Session storage: model, persistence, and locking.

mod lock;
mod model;
mod storage;

pub use lock::{acquire_origin_lock, SessionLock};
pub use model::{now_secs, session_key, SessionRecord, SessionStatus};
pub use storage::{
    delete_session, delete_session_by_channel_id, list_sessions, load_session,
    load_session_by_channel_id, save_session, take_store_diagnostics,
    update_session_close_state_by_channel_id, SessionStoreDiagnostics,
};
