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

use std::env;

use crate::snapshot::Snapshot;

mod icon;
mod snapshot;

fn main() -> std::io::Result<()> {
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

    let debug = args.len() == 2 && &args[1] == "--debug";

    let mut socket = niri_ipc::socket::Socket::connect()?;

    let workspaces = match socket.send(niri_ipc::Request::Workspaces).unwrap().unwrap() {
        niri_ipc::Response::Workspaces(w) => w,
        r @ _ => panic!("Expected workspaces but got {r:?}"),
    };
    let windows = match socket.send(niri_ipc::Request::Windows).unwrap().unwrap() {
        niri_ipc::Response::Windows(w) => w,
        r @ _ => panic!("Expected windows but got {r:?}"),
    };

    let mut snapshot = Snapshot::new(workspaces, windows);
    snapshot.print();

    let reply = socket.send(niri_ipc::Request::EventStream)?;
    if let Ok(niri_ipc::Response::Handled) = reply {
        let mut read_event = socket.read_events();
        while let Ok(evt) = read_event() {
            if debug {
                eprintln!("{:?}", &evt);
            }
            if snapshot.update(evt) {
                snapshot.print();
            }
        }
    }
    Ok(())
}
