//! Plugin state. The plugin runs on one thread, but host calls may enter it
//! again (an event fired from inside a call), so the state is only borrowed for
//! short sections without host calls. A failed borrow returns `None` instead of
//! panicking: a panic would disable the plugin and let everyone in.

use std::cell::RefCell;

use pumbo_bans_core::Engine;

pub struct State {
    pub engine: Engine,
    pub data_dir: String,
    /// When addresses past the retention period were last forgotten.
    pub last_forget_ms: u64,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

pub fn install(state: State) {
    STATE.with(|c| {
        if let Ok(mut g) = c.try_borrow_mut() {
            *g = Some(state);
        }
    });
}

/// Runs `f` with exclusive access to the state. Never call the host inside `f`.
pub fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|c| c.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}
