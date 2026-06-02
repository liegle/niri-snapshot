use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
};

use crate::state::State;

pub enum Update {
    Consume,
    Ignore,
    Cache,
}

pub struct Snapshot(Arc<Mutex<State>>);
// SAFETY: Any Rc, Weak and RefCell inside State must be used in lock() scope
// and should never escape from it
unsafe impl Send for Snapshot {}
unsafe impl Sync for Snapshot {}

impl Snapshot {
    pub fn new(workspaces: Vec<niri_ipc::Workspace>, windows: Vec<niri_ipc::Window>) -> Self {
        Self(Arc::new(Mutex::new(State::new(workspaces, windows))))
    }

    pub fn lock_print(&self) {
        let stdout = io::stdout().lock();
        serde_json::to_writer::<_, State>(stdout, &self.0.lock().unwrap()).unwrap();
        let mut stdout = io::stdout().lock();
        stdout.write_all(&[b'\n']).unwrap();
        stdout.flush().unwrap();
    }

    #[cfg(feature = "verify")]
    pub fn lock_verify(&self, state: &niri_ipc::state::EventStreamState) {
        self.0.lock().unwrap().verify(state);
    }

    pub fn lock_update(&mut self, evt: &niri_ipc::Event) -> Update {
        self.0.lock().unwrap().update(evt)
    }
}

impl Clone for Snapshot {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}
