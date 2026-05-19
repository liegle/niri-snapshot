use std::{
    cell::RefCell,
    collections::HashMap,
    convert::identity,
    rc::{Rc, Weak},
};

use crate::icon;

pub struct Snapshot {
    outputs: HashMap<String, Vec<Ptr<Workspace>>>,
    workspaces: indexmap::IndexMap<u64, Rc<RefCell<Workspace>>>,
    windows: indexmap::IndexMap<u64, Rc<RefCell<Window>>>,
    icon_cache: icon::IconCache,
    workspace_change_received: bool,
    window_change_received: bool,
}

#[derive(serde::Serialize, Debug)]
struct Workspace {
    id: u64,
    active: bool,
    focused: bool,
    urgent: bool,
    columns: Vec<Vec<Ptr<Window>>>,
    floatings: Vec<Ptr<Window>>,
    // not serialize fields
    #[serde(skip_serializing)]
    output: String,
    #[serde(skip_serializing)]
    active_window_id: Option<u64>,
}

#[derive(serde::Serialize, Clone, Debug)]
struct Window {
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    active: bool,
    focused: bool,
    urgent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    // not serialize fields
    #[serde(skip_serializing)]
    workspace_id: u64,
    #[serde(skip_serializing)]
    layout: Option<(usize, usize)>,
}

#[derive(Clone, Debug)]
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
            workspaces: indexmap::IndexMap::new(),
            windows: indexmap::IndexMap::new(),
            icon_cache: icon::IconCache::new(),
            workspace_change_received: false,
            window_change_received: false,
        };

        for window in windows {
            let Some(workspace_id) = window.workspace_id else {
                eprintln!("Found window without workspace id {:?}", window.title);
                continue;
            };
            if !window.is_floating && window.layout.pos_in_scrolling_layout.is_none() {
                eprintln!("Found window without pos nor floating {:?}", window.title);
                continue;
            }
            snapshot.windows.insert(
                window.id,
                Rc::new(RefCell::new(Window {
                    id: window.id,
                    title: window.title,
                    active: false,
                    focused: window.is_focused,
                    urgent: window.is_urgent,
                    icon: snapshot.icon_cache.lookup(window.app_id),
                    workspace_id: workspace_id,
                    layout: window
                        .layout
                        .pos_in_scrolling_layout
                        .map(|(x, y)| (x - 1, y - 1)),
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
                    floatings: Vec::new(),
                    output: workspace.output.unwrap_or(String::new()),
                    active_window_id: workspace.active_window_id,
                })),
            );
        }

        let mut workspace_windows = HashMap::<_, (Vec<_>, Vec<Vec<_>>)>::new();
        for (_, window) in snapshot.windows.iter() {
            let (floatings, columns) = workspace_windows
                .entry(window.borrow().workspace_id)
                .or_default();
            if let Some((x, y)) = window.borrow().layout {
                if columns.len() < x {
                    columns.resize(x, Vec::new());
                }
                let column = &mut columns[x - 1];
                if column.len() < y {
                    column.resize(y, None);
                }
                column[y - 1] = Some(Ptr(Rc::downgrade(&window)));
            } else {
                floatings.push(Some(Ptr(Rc::downgrade(&window))));
            }
        }

        for (id, workspace) in snapshot.workspaces.iter() {
            let _ = snapshot
                .outputs
                .entry(workspace.borrow().output.clone())
                .or_default()
                .push_mut(Ptr(Rc::downgrade(&workspace)));
            if let Some((floatings, columns)) = workspace_windows.remove(id) {
                let mut workspace = workspace.borrow_mut();
                workspace.columns = columns
                    .into_iter()
                    .map(|v| v.into_iter().filter_map(identity).collect())
                    .collect();
                workspace.floatings = floatings.into_iter().filter_map(identity).collect();
            }

            let id = workspace.borrow().active_window_id;
            if let Some(id) = id {
                let mut workspace = workspace.borrow_mut();
                workspace.columns.iter_mut().for_each(|column| {
                    column.iter_mut().for_each(|window| {
                        if let Some(window) = window.0.upgrade() {
                            let active = id == window.borrow().id;
                            window.borrow_mut().active = active;
                        }
                    })
                });
                workspace.floatings.iter_mut().for_each(|window| {
                    if let Some(window) = window.0.upgrade() {
                        let active = id == window.borrow().id;
                        window.borrow_mut().active = active;
                    }
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
                if !self.workspace_change_received {
                    self.workspace_change_received = true;
                } else {
                    eprintln!("Assumed not possible to receive but received WorkspacesChanged");
                }
                false
            }
            niri_ipc::Event::WorkspaceUrgencyChanged { id, urgent } => {
                match self.workspaces.get(&id) {
                    Some(workspace) => {
                        workspace.borrow_mut().urgent = urgent;
                        true
                    }
                    None => {
                        eprintln!("Received WorkspaceUrgencyChanged id not found: {id}");
                        false
                    }
                }
            }
            niri_ipc::Event::WorkspaceActivated { id, focused } => match self.workspaces.get(&id) {
                Some(workspace) => {
                    workspace.borrow_mut().focused = focused;
                    true
                }
                None => {
                    eprintln!("Received WorkspaceActivated id not found: {id}");
                    false
                }
            },
            niri_ipc::Event::WorkspaceActiveWindowChanged {
                workspace_id,
                active_window_id,
            } => match self.workspaces.get(&workspace_id) {
                Some(workspace) => {
                    for column in &workspace.borrow_mut().columns {
                        for window in column {
                            if let Some(window) = window.0.upgrade() {
                                window.borrow_mut().active = false;
                            }
                        }
                    }
                    for window in &workspace.borrow_mut().floatings {
                        if let Some(window) = window.0.upgrade() {
                            window.borrow_mut().active = false;
                        }
                    }
                    if let Some(active_window_id) = active_window_id {
                        if let Some(window) = self.windows.get(&active_window_id) {
                            window.borrow_mut().active = true;
                        } else {
                            eprintln!("Active window id {active_window_id} not found");
                        }
                    }
                    true
                }
                None => {
                    eprintln!("Received WorkspaceActiveWindowChanged id not found: {workspace_id}");
                    false
                }
            },
            niri_ipc::Event::WindowsChanged { windows: _ } => {
                if !self.window_change_received {
                    self.window_change_received = true;
                } else {
                    eprintln!("Assumed not possible to receive but received WindowsChanged");
                }
                false
            }
            niri_ipc::Event::WindowOpenedOrChanged { window } => {
                if let Some(workspace_id) = window.workspace_id {
                    if let Some(found_window) = self.windows.get(&window.id) {
                        // Changed
                        let mut found_window_ref = found_window.borrow_mut();
                        found_window_ref.title = window.title.clone();
                        found_window_ref.focused = window.is_focused;
                        found_window_ref.urgent = window.is_urgent;
                        found_window_ref.icon = self.icon_cache.lookup(window.app_id);
                        let layout = window
                            .layout
                            .pos_in_scrolling_layout
                            .map(|(x, y)| (x - 1, y - 1));
                        if found_window_ref.workspace_id != workspace_id {
                            let old_workspace_id = found_window_ref.workspace_id;
                            let old_layout = found_window_ref.layout;
                            found_window_ref.workspace_id = workspace_id;
                            found_window_ref.layout = layout;

                            let Some(old_workspace) = self.workspaces.get_mut(&old_workspace_id)
                            else {
                                eprintln!(
                                    "Old workspace id {old_workspace_id} of window {:?} not found",
                                    window.title
                                );
                                return true;
                            };
                            remove_from_old_workspace(
                                old_workspace,
                                &found_window_ref,
                                &old_layout,
                            );
                            let Some(workspace) = self.workspaces.get_mut(&workspace_id) else {
                                eprintln!(
                                    "Workspace id {workspace_id} of window {:?} not found",
                                    found_window_ref.title
                                );
                                return true;
                            };
                            add_into_workspace(
                                workspace,
                                Ptr(Rc::downgrade(found_window)),
                                &found_window_ref.layout,
                            );
                        } else if found_window_ref.layout != layout {
                            let old_layout = found_window_ref.layout;
                            found_window_ref.layout = layout;

                            let Some(workspace) = self.workspaces.get_mut(&workspace_id) else {
                                eprintln!(
                                    "Workspace id {workspace_id} of window {:?} not found",
                                    found_window_ref.title
                                );
                                return true;
                            };
                            remove_from_old_workspace(workspace, &found_window_ref, &old_layout);
                            add_into_workspace(
                                workspace,
                                Ptr(Rc::downgrade(found_window)),
                                &found_window_ref.layout,
                            );
                        }
                        true
                    } else {
                        // Opened
                        true
                    }
                } else {
                    eprintln!("Received WindowOpenedOrChanged with None workspace_id");
                    false
                }
            }
            _ => false,
        }
    }
}

fn remove_from_old_workspace(
    old_workspace: &mut Rc<RefCell<Workspace>>,
    window: &Window,
    old_layout: &Option<(usize, usize)>,
) {
    if let Some((x, y)) = *old_layout {
        let columns = &mut old_workspace.borrow_mut().columns;
        let Some(column) = columns.get_mut(x) else {
            eprintln!(
                "Window {:?} column {} not found in workspace {}",
                window.title,
                x,
                old_workspace.borrow().id
            );
            return;
        };
        if column.len() <= y {
            eprintln!(
                "Window {:?} row {} not found in column {}",
                window.title, y, x
            );
            return;
        }
        column.remove(y);
        if column.is_empty() {
            columns.remove(x);
            for column in &mut columns[x..] {
                for window in column {
                    if let Some(window) = window.0.upgrade() {
                        if let Some((x, _)) = window.borrow_mut().layout.as_mut() {
                            *x = *x - 1;
                        }
                    }
                }
            }
        } else {
            for window in &mut column[y..] {
                if let Some(window) = window.0.upgrade() {
                    if let Some((_, y)) = window.borrow_mut().layout.as_mut() {
                        *y = *y - 1;
                    }
                }
            }
        }
    } else {
        let floatings = &mut old_workspace.borrow_mut().floatings;
        floatings.retain(|w| w.0.upgrade().is_some_and(|w| w.borrow().id != window.id));
    }
}

fn add_into_workspace(
    workspace: &mut Rc<RefCell<Workspace>>,
    window: Ptr<Window>,
    layout: &Option<(usize, usize)>,
) {
    if let Some((x, y)) = *layout {
        let columns = &mut workspace.borrow_mut().columns;
        let column = if let Some(column) = columns.get_mut(x) {
            column
        } else {
            columns.push_mut(Vec::new())
        };
        column.insert(y, window);
    } else {
        let floatings = &mut workspace.borrow_mut().floatings;
        floatings.push(window);
    }
}
