use std::{
    cell::RefCell,
    collections::HashMap,
    convert::identity,
    rc::{Rc, Weak},
};

use gtk::glib::clone::Downgrade;

use crate::icon;

pub struct Snapshot {
    outputs: HashMap<String, Vec<Ptr<Workspace>>>,
    workspaces: HashMap<u64, Rc<RefCell<Workspace>>>,
    windows: HashMap<u64, Rc<RefCell<Window>>>,
    icon_cache: icon::IconCache,
}

#[derive(serde::Serialize)]
struct Workspace {
    id: u64,
    active: bool,
    focused: bool,
    urgent: bool,
    columns: Vec<Vec<Ptr<Window>>>,
    #[serde(skip_serializing)]
    output: String,
    #[serde(skip_serializing)]
    active_window_id: Option<u64>,
}

#[derive(serde::Serialize, Clone)]
struct Window {
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    active: bool,
    focused: bool,
    urgent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    #[serde(skip_serializing)]
    layout: Option<(usize, usize)>,
}

#[derive(Clone)]
struct Ptr<T>(Weak<RefCell<T>>);

impl<T: serde::Serialize> serde::Serialize for Ptr<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self.0.upgrade() {
            Some(c) => c.borrow().serialize(serializer),
            None => serializer.serialize_none(),
        }
    }
}

impl Snapshot {
    pub fn new(mut workspaces: Vec<niri_ipc::Workspace>, windows: Vec<niri_ipc::Window>) -> Self {
        let mut snapshot = Self {
            outputs: HashMap::new(),
            workspaces: HashMap::new(),
            windows: HashMap::new(),
            icon_cache: icon::IconCache::new(),
        };

        for window in windows {
            let Some(workspace_id) = window.workspace_id else {
                eprintln!("Found window without workspace id {:?}", window.title);
                continue;
            };
            if !window.is_floating && window.layout.pos_in_scrolling_layout.is_none() {
                eprintln!(
                    "Found window without pos while is not floating {:?}",
                    window.title
                );
                continue;
            }
            snapshot.windows.insert(
                workspace_id,
                Rc::new(RefCell::new(Window {
                    id: window.id,
                    title: window.title,
                    active: false,
                    focused: window.is_focused,
                    urgent: window.is_urgent,
                    icon: snapshot.icon_cache.lookup(window.app_id),
                    layout: window.layout.pos_in_scrolling_layout,
                })),
            );
        }

        workspaces.sort_by_cached_key(|workspace| workspace.idx);
        for workspace in workspaces {
            snapshot.workspaces.insert(
                workspace.id,
                Rc::new(RefCell::new(Workspace {
                    id: workspace.id,
                    urgent: workspace.is_urgent,
                    active: workspace.is_active,
                    focused: workspace.is_focused,
                    columns: Vec::new(),
                    output: workspace.output.unwrap_or(String::new()),
                    active_window_id: workspace.active_window_id,
                })),
            );
        }

        let mut workspace_windows = HashMap::<_, (Vec<_>, Vec<Vec<_>>)>::new();
        for (id, window) in snapshot.windows.iter() {
            let columns = workspace_windows.entry(id).or_default();
            if let Some((x, y)) = window.borrow().layout {
                if columns.1.len() < x {
                    columns.1.resize(x, Vec::new());
                }
                let column = &mut columns.1[x - 1];
                if column.len() < y {
                    column.resize(y, None);
                }
                columns.1[x - 1][y - 1] = Some(Ptr(window.downgrade()));
            } else {
                columns.0.push(Some(Ptr(window.downgrade())));
            }
        }

        let mut workspace_windows = workspace_windows
            .into_iter()
            .map(|(k, mut v)| {
                if !v.0.is_empty() {
                    v.1.insert(0, v.0);
                }
                (
                    k,
                    v.1.into_iter()
                        .map(|column| column.into_iter().filter_map(identity).collect::<Vec<_>>())
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<HashMap<_, _>>();

        for (id, workspace) in snapshot.workspaces.iter() {
            let _ = snapshot
                .outputs
                .entry(workspace.borrow().output.clone())
                .or_default()
                .push_mut(Ptr(workspace.downgrade()));
            if let Some(columns) = workspace_windows.remove(id) {
                workspace.borrow_mut().columns = columns;
            }
            if let Some(id) = workspace.borrow().active_window_id {
                workspace
                    .borrow_mut()
                    .columns
                    .iter_mut()
                    .for_each(|column| {
                        column.iter_mut().for_each(|window| {
                            if let Some(window) = window.0.upgrade() {
                                window.borrow_mut().active = id == window.borrow().id
                            }
                        })
                    });
            }
        }

        snapshot
    }

    pub fn print(&self) {
        println!("{}", serde_json::to_string_pretty(&self.outputs).unwrap());
    }

    pub fn update(&mut self, evt: niri_ipc::Event) -> bool {
        match evt {
            niri_ipc::Event::WorkspacesChanged { workspaces: _ } => {
                eprintln!("Assumed not possible to receive but received WorkspacesChanged");
                false
            }
            niri_ipc::Event::WorkspaceUrgencyChanged { id, urgent } => true,
            _ => false,
        }
    }
}
