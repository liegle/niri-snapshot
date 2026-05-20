use std::{
    cell::RefCell,
    collections::HashMap,
    convert::identity,
    rc::{Rc, Weak},
};

use crate::icon;

#[derive(serde::Serialize)]
pub struct Snapshot {
    outputs: HashMap<String, Output>,
    focused_workspace_id: Option<u64>,
    focused_window_id: Option<u64>,

    // not serialize fields
    #[serde(skip)]
    workspaces: indexmap::IndexMap<u64, Rc<RefCell<Workspace>>>,
    #[serde(skip)]
    windows: indexmap::IndexMap<u64, Rc<RefCell<Window>>>,
    #[serde(skip)]
    icon_cache: icon::IconCache,
}

#[derive(serde::Serialize, Debug, Default)]
struct Output {
    workspaces: Vec<Ptr<Workspace>>,
    active_workspace_id: Option<u64>,
}

#[derive(serde::Serialize, Debug)]
struct Workspace {
    id: u64,
    urgent: bool,
    columns: Vec<Vec<Ptr<Window>>>,
    floatings: Vec<Ptr<Window>>,
    active_window_id: Option<u64>,

    // not serialize fields
    #[serde(skip)]
    output: String,
}

#[derive(serde::Serialize, Clone, Debug)]
struct Window {
    id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    urgent: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon: Option<String>,

    // not serialize fields
    #[serde(skip)]
    workspace_id: u64,
    #[serde(skip)]
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
            focused_workspace_id: None,
            focused_window_id: None,
            workspaces: indexmap::IndexMap::new(),
            windows: indexmap::IndexMap::new(),
            icon_cache: icon::IconCache::new(),
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
            if window.is_focused {
                snapshot.focused_window_id = Some(window.id);
            }
            snapshot.windows.insert(
                window.id,
                Rc::new(RefCell::new(Window {
                    id: window.id,
                    title: window.title,
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
            if workspace.is_focused {
                snapshot.focused_workspace_id = Some(workspace.id);
            }
            snapshot.workspaces.insert(
                workspace.id,
                Rc::new(RefCell::new(Workspace {
                    id: workspace.id,
                    urgent: workspace.is_urgent,
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
            snapshot
                .outputs
                .entry(workspace.borrow().output.clone())
                .or_default()
                .workspaces
                .push(Ptr(Rc::downgrade(&workspace)));
            if let Some((floatings, columns)) = workspace_windows.remove(id) {
                let mut workspace = workspace.borrow_mut();
                workspace.columns = columns
                    .into_iter()
                    .map(|v| v.into_iter().filter_map(identity).collect())
                    .collect();
                workspace.floatings = floatings.into_iter().filter_map(identity).collect();
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
                // TODO
                false
            }
            niri_ipc::Event::WorkspaceUrgencyChanged { id, urgent } => {
                let Some(workspace) = self.workspaces.get_mut(&id) else {
                    eprintln!("Received WorkspaceUrgencyChanged id not found: {id}");
                    return false;
                };
                workspace.borrow_mut().urgent = urgent;
                true
            }
            niri_ipc::Event::WorkspaceActivated { id, focused } => {
                let Some(workspace) = self.workspaces.get(&id) else {
                    eprintln!("Received WorkspaceActivated id not found: {id}");
                    return false;
                };
                let Some(output) = self.outputs.get_mut(&workspace.borrow().output) else {
                    eprintln!("Received WorkspaceActivated but workspace's output not found");
                    return false;
                };
                output.active_workspace_id = Some(id);
                if focused {
                    self.focused_workspace_id = Some(id);
                }
                true
            }
            niri_ipc::Event::WorkspaceActiveWindowChanged {
                workspace_id,
                active_window_id,
            } => {
                let Some(workspace) = self.workspaces.get(&workspace_id) else {
                    eprintln!("Received WorkspaceActiveWindowChanged id not found: {workspace_id}");
                    return false;
                };
                workspace.borrow_mut().active_window_id = active_window_id;
                true
            }
            niri_ipc::Event::WindowsChanged { windows: _ } => {
                // TODO
                false
            }
            niri_ipc::Event::WindowOpenedOrChanged { window } => {
                let Some(workspace_id) = window.workspace_id else {
                    eprintln!("Received WindowOpenedOrChanged with None workspace_id");
                    return false;
                };

                match self.windows.get(&window.id) {
                    // Changed
                    // only for workspace_id / is_floating / title / app_id
                    Some(found_window) => {
                        let mut found_window_ref = found_window.borrow_mut();
                        found_window_ref.title = window.title.clone();
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
                            append_to_workspace(
                                workspace,
                                Ptr(Rc::downgrade(found_window)),
                                &found_window_ref.layout,
                            );
                        } else if found_window_ref.layout.is_none() != window.is_floating {
                            
                        }
                        true
                    }
                    // Opened
                    None => true,
                }
            }
            _ => false,
        }
    }
}

/// Used to process window removement when window is moved from one workspace
/// to anothor. Only operates one window and onw column.
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
        }
    } else {
        let floatings = &mut old_workspace.borrow_mut().floatings;
        floatings.retain(|w| w.0.upgrade().is_some_and(|w| w.borrow().id != window.id));
    }
}

/// Used to process window appending when window is moved from one workspace
/// to anothor. Only operates one window and onw column.
fn append_to_workspace(
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
