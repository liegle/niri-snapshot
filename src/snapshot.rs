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
    workspaces: HashMap<u64, Rc<RefCell<Workspace>>>,
    #[serde(skip)]
    windows: HashMap<u64, Rc<RefCell<Window>>>,
    #[serde(skip)]
    icon_cache: icon::IconCache,
}

#[derive(serde::Serialize, Debug, Default)]
struct Output {
    workspaces: Vec<Ptr<Workspace>>,
    active_workspace_id: Option<u64>,
}

#[derive(serde::Serialize, Debug, Default)]
struct Workspace {
    id: u64,
    urgent: bool,
    columns: Vec<Vec<Ptr<Window>>>,
    floatings: Vec<Ptr<Window>>,
    active_window_id: Option<u64>,

    // not serialize fields
    #[serde(skip)]
    idx: u8,
    #[serde(skip)]
    output: String,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
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
    pub fn new(workspaces: Vec<niri_ipc::Workspace>, windows: Vec<niri_ipc::Window>) -> Self {
        let (workspaces, focused_workspace_id) = init_workspaces(workspaces);
        let mut icon_cache = icon::IconCache::new();
        let (windows, focused_window_id) = init_windows(windows, &mut icon_cache);
        let outputs = init_outputs(&workspaces, &windows);

        Self {
            outputs,
            focused_workspace_id,
            focused_window_id,
            workspaces,
            windows,
            icon_cache,
        }
    }

    pub fn print(&self) {
        println!("{}", serde_json::to_string_pretty(&self).unwrap());
    }

    pub fn update(&mut self, evt: niri_ipc::Event) -> bool {
        // For all `entry(xxx).or_default()`s in this function:
        // `or_default` is used to create a dummy entry here to prevent
        // wrong event sending order, in which case this entry will be
        // modified by next events.
        match evt {
            niri_ipc::Event::WorkspacesChanged { workspaces } => {
                let (workspaces, focused_workspace_id) = init_workspaces(workspaces);
                let outputs = init_outputs(&workspaces, &self.windows);
                self.outputs = outputs;
                self.focused_workspace_id = focused_workspace_id;
                self.workspaces = workspaces;
                false
            }
            niri_ipc::Event::WorkspaceUrgencyChanged { id, urgent } => {
                self.workspaces.entry(id).or_default().borrow_mut().urgent = urgent;
                true
            }
            niri_ipc::Event::WorkspaceActivated { id, focused } => {
                // Can't create dummy output so we have to throw an error
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
                self.workspaces
                    .entry(workspace_id)
                    .or_default()
                    .borrow_mut()
                    .active_window_id = active_window_id;
                true
            }
            niri_ipc::Event::WindowsChanged { windows: _ } => {
                // Ignored because this will only be sent on init
                false
            }
            niri_ipc::Event::WindowOpenedOrChanged { window } => {
                let Some(workspace_id) = window.workspace_id else {
                    eprintln!("Received WindowOpenedOrChanged with None workspace_id");
                    return false;
                };

                if let Some(found_window) = self.windows.get(&window.id) {
                    // Changed
                    // only for workspace_id / is_floating / title / app_id
                    // Or opened but was sent in wrong order
                    let mut found_window_ref = found_window.borrow_mut();
                    found_window_ref.title = window.title.clone();
                    found_window_ref.icon = self.icon_cache.lookup(window.app_id);
                    let layout = window
                        .layout
                        .pos_in_scrolling_layout
                        .map(|(x, y)| (x - 1, y - 1));
                    if found_window_ref.workspace_id != workspace_id {
                        let old_workspace = self
                            .workspaces
                            .entry(found_window_ref.workspace_id)
                            .or_default();
                        remove_from_old_workspace(old_workspace, &found_window_ref);
                        found_window_ref.layout = layout;
                        let workspace = self.workspaces.entry(workspace_id).or_default();
                        add_to_workspace(workspace, found_window);
                    } else if found_window_ref.layout.is_none() != window.is_floating {
                        let workspace = self.workspaces.entry(workspace_id).or_default();
                        remove_from_old_workspace(workspace, &found_window_ref);
                        found_window_ref.layout = layout;
                        add_to_workspace(workspace, found_window);
                    }
                    true
                } else {
                    // Opened
                    let window_ptr = Rc::new(RefCell::new(Window {
                        id: window.id,
                        title: window.title,
                        urgent: window.is_urgent,
                        icon: self.icon_cache.lookup(window.app_id),
                        workspace_id: workspace_id,
                        layout: window.layout.pos_in_scrolling_layout,
                    }));
                    let workspace = self.workspaces.entry(workspace_id).or_default();
                    add_to_workspace(workspace, &window_ptr);
                    self.windows.insert(window.id, window_ptr);
                    true
                }
            }
            niri_ipc::Event::WindowClosed { id } => {
                if let Some(window) = self.windows.remove(&id) {
                    let workspace = self
                        .workspaces
                        .entry(window.borrow().workspace_id)
                        .or_default();
                    remove_from_old_workspace(workspace, &window.borrow());
                    true
                } else {
                    eprintln!("Can't find window to be closed: {id}");
                    false
                }
            }
            niri_ipc::Event::WindowFocusChanged { id } => {
                self.focused_window_id = id;
                // TODO: maybe i should check whether this window exists?
                true
            }
            niri_ipc::Event::WindowUrgencyChanged { id, urgent } => {
                self.windows.entry(id).or_default().borrow_mut().urgent = urgent;
                true
            }
            niri_ipc::Event::WindowLayoutsChanged { changes } => {
                for change in changes {
                    let window = self.windows.entry(change.0).or_default();
                    let workspace = self.workspaces.entry(window.borrow().workspace_id).or_default();
                    remove_from_old_workspace(workspace, &window.borrow());
                    add_to_workspace(workspace, window);
                }
                true
            }
            _ => false,
        }
    }
}

/// Create workspace map from workspace vec sent by niri
fn init_workspaces(
    workspaces: Vec<niri_ipc::Workspace>,
) -> (HashMap<u64, Rc<RefCell<Workspace>>>, Option<u64>) {
    let mut result = HashMap::new();
    let mut focused = None;
    for workspace in workspaces {
        if workspace.is_focused {
            focused = Some(workspace.id);
        }
        result.insert(
            workspace.id,
            Rc::new(RefCell::new(Workspace {
                id: workspace.id,
                urgent: workspace.is_urgent,
                columns: Vec::new(),
                floatings: Vec::new(),
                active_window_id: workspace.active_window_id,
                idx: workspace.idx,
                output: workspace.output.unwrap_or(String::new()),
            })),
        );
    }
    (result, focused)
}

/// Create windows map from windows vec sent by niri
fn init_windows(
    windows: Vec<niri_ipc::Window>,
    icon_cache: &mut icon::IconCache,
) -> (HashMap<u64, Rc<RefCell<Window>>>, Option<u64>) {
    let mut result = HashMap::new();
    let mut focused = None;
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
            focused = Some(window.id);
        }
        result.insert(
            window.id,
            Rc::new(RefCell::new(Window {
                id: window.id,
                title: window.title,
                urgent: window.is_urgent,
                icon: icon_cache.lookup(window.app_id),
                workspace_id: workspace_id,
                layout: window
                    .layout
                    .pos_in_scrolling_layout
                    .map(|(x, y)| (x - 1, y - 1)),
            })),
        );
    }
    (result, focused)
}

/// Create outputs map from workspaces and windows map created or stored
fn init_outputs(
    workspaces: &HashMap<u64, Rc<RefCell<Workspace>>>,
    windows: &HashMap<u64, Rc<RefCell<Window>>>,
) -> HashMap<String, Output> {
    let mut workspace_windows = HashMap::<_, (Vec<_>, Vec<Vec<_>>)>::new();
    for window in windows.values() {
        let (floatings, columns) = workspace_windows
            .entry(window.borrow().workspace_id)
            .or_default();
        if let Some((x, y)) = window.borrow().layout {
            if columns.len() <= x {
                columns.resize(x + 1, Vec::new());
            }
            let column = &mut columns[x];
            if column.len() <= y {
                column.resize(y + 1, None);
            }
            column[y] = Some(Ptr(Rc::downgrade(&window)));
        } else {
            floatings.push(Some(Ptr(Rc::downgrade(&window))));
        }
    }

    let mut outputs = HashMap::<_, Output>::new();
    for (id, workspace) in workspaces.iter() {
        outputs
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

    for output in outputs.values_mut() {
        output
            .workspaces
            .sort_by_key(|workspace| match workspace.0.upgrade() {
                Some(workspace) => workspace.borrow().idx,
                None => u8::MAX,
            });
    }
    outputs
}

/// Remove window from a workspace at its layout with [`remove`]
///
/// [`remove`]: Vec::remove
fn remove_from_old_workspace(workspace: &mut Rc<RefCell<Workspace>>, window: &Window) {
    if let Some((x, y)) = window.layout {
        let columns = &mut workspace.borrow_mut().columns;
        let Some(column) = columns.get_mut(x) else {
            eprintln!(
                "Window {:?} column {} not found in workspace {}",
                window.title,
                x,
                workspace.borrow().id
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
    } else {
        let floatings = &mut workspace.borrow_mut().floatings;
        floatings.retain(|w| w.0.upgrade().is_some_and(|w| w.borrow().id != window.id));
    }
}

/// Add a window to a workspace at a new layout with [`resize`] and [`index_mut`]
///
/// [`resize`]: Vec::resize
/// [`index_mut`]: std::ops::IndexMut::index_mut
fn add_to_workspace(workspace: &mut Rc<RefCell<Workspace>>, window: &Rc<RefCell<Window>>) {
    if let Some((x, y)) = window.borrow().layout {
        let columns = &mut workspace.borrow_mut().columns;
        if columns.len() <= x {
            columns.resize(x + 1, Vec::new());
        }
        let column = &mut columns[x];
        if column.len() <= y {
            column.resize(y + 1, Ptr(Weak::new()));
        }
        column[y] = Ptr(Rc::downgrade(&window));
    } else {
        let floatings = &mut workspace.borrow_mut().floatings;
        floatings.push(Ptr(Rc::downgrade(&window)));
    }
}
