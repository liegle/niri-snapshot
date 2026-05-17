use crate::snapshot::Snapshot;

/// ```json
/// "HDMI-A-1": [
///     {
///         id: 8, // u64
///         urgent: false,
///         active: false,
///         focused: false,
///         columns: [
///             [
///                 {
///                     id: 55, // u64
///                     title: "firefox",
///                     focused: false,
///                     urgent: false,
///                     icon: "path/to/icon"
///                 }
///             ]
///         ]
///     }
/// ]
/// ```
mod icon;
mod snapshot;

fn main() -> std::io::Result<()> {
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
            if snapshot.update(evt) {
                snapshot.print();
            }
        }
    }
    Ok(())
}
