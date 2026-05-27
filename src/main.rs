//! Print niri current state like:
//! ```json
//! {
//!     "focused_workspace_id": 4, // u64
//!     "focused_window_id": 15, // u64
//!     "HDMI-A-1": [
//!         {
//!             id: 8, // u64
//!             active_window_id: 2, // u64
//!             urgent: false,
//!             active: false,
//!             columns: [
//!                 [
//!                     {
//!                         id: 55, // u64
//!                         title: "firefox",
//!                         urgent: false,
//!                         icon: "path/to/icon"
//!                     }
//!                 ]
//!             ],
//!             floatings: []
//!         }
//!     ]
//! }
//! ```
//!
//! Or use `niri-snapshot ws 3` to switch to workspace id 3
//! because niri msg doesn't work until this [`issue`] is solved
//! [`issue`]: https://github.com/niri-wm/niri/issues/647

use std::{
    env,
    io::{self, Error, ErrorKind},
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::snapshot::Snapshot;

mod icon;
mod snapshot;

fn main() -> io::Result<()> {
    let args = env::args().collect::<Vec<String>>();
    if args.len() >= 3 && &args[1] == "ws" {
        let id = args[2].parse::<u64>().unwrap();
        let mut socket = niri_ipc::socket::Socket::connect()?;
        let _ = socket.send(niri_ipc::Request::Action(
            niri_ipc::Action::FocusWorkspace {
                reference: niri_ipc::WorkspaceReferenceArg::Id(id),
            },
        ))?;
        return Ok(());
    }

    let mut socket = niri_ipc::socket::Socket::connect()?;
    let workspaces = match socket.send(niri_ipc::Request::Workspaces).unwrap().unwrap() {
        niri_ipc::Response::Workspaces(w) => w,
        r => panic!("Expected workspaces but got {r:?}"),
    };
    let windows = match socket.send(niri_ipc::Request::Windows).unwrap().unwrap() {
        niri_ipc::Response::Windows(w) => w,
        r => panic!("Expected windows but got {r:?}"),
    };

    let Ok(niri_ipc::Response::Handled) = socket.send(niri_ipc::Request::EventStream)? else {
        return Err(Error::new(
            ErrorKind::ConnectionRefused,
            "Failed to connect to event stream",
        ));
    };
    let snapshot = Arc::new(Mutex::new(Snapshot::new(workspaces, windows)));
    snapshot.lock().unwrap().print();
    let (tx, rx) = mpsc::channel();
    let handle = print_loop(rx, Arc::clone(&snapshot));
    update_loop(socket, tx, snapshot);
    handle.join().unwrap();
    Ok(())
}

const THROTTLE_DURATION: Duration = Duration::from_millis(3);

fn update_loop(
    socket: niri_ipc::socket::Socket,
    tx: Sender<Instant>,
    snapshot: Arc<Mutex<Snapshot>>,
) {
    #[cfg(feature = "verify")]
    let mut state = niri_ipc::state::EventStreamState::default();
    let mut cache = Vec::new();
    let mut read_event = socket.read_events();
    let mut counter = 0;
    while let Ok(evt) = read_event() {
        #[cfg(feature = "verify")]
        {
            use niri_ipc::state::EventStreamStatePart;
            state.apply(evt.clone());
            eprintln!(" ==> \x1B[34m{:?}\x1B[0m", evt);
        }
        // Skip first WorkspacesChanged and WindowsChanged
        if counter <= 1 {
            counter += 1;
            continue;
        }
        let consume = { snapshot.lock().unwrap().update(&evt) };
        match consume {
            Update::Consume => {
                let mut used = true;
                while used {
                    used = false;
                    cache.retain_mut(|evt| match snapshot.lock().unwrap().update(&evt) {
                        Update::Consume | Update::Ignore => {
                            used = true;
                            false
                        }
                        Update::Cache => true,
                    });
                }
                tx.send(Instant::now() + THROTTLE_DURATION).unwrap();
                #[cfg(feature = "verify")]
                {
                    eprintln!("\x1B[33m{} caches left\x1B[0m", cache.len());
                    snapshot.lock().unwrap().verify(&state);
                }
            }
            Update::Cache => {
                let _ = cache.push(evt);
            }
            _ => (),
        }
    }
}

fn print_loop(rx: Receiver<Instant>, snapshot: Arc<Mutex<Snapshot>>) -> JoinHandle<()> {
    thread::spawn(move || {
        while let Ok(mut time) = rx.recv() {
            let mut now = Instant::now();
            while time > now {
                thread::sleep(time - now);
                while let Ok(next) = rx.try_recv() {
                    time = if next > time { next } else { time }
                }
                now = Instant::now();
            }
            {
                let snapshot = snapshot.lock().unwrap();
                snapshot.print();
            }
        }
    })
}

enum Update {
    Consume,
    Ignore,
    Cache,
}
