use std::{
    collections::HashMap,
    convert::identity,
    io::{self, Write},
    sync::{Arc, Mutex, Weak},
};

use crate::{Update, icon::IconCache};

#[derive(serde::Serialize)]
pub struct Snapshot {
    outputs: HashMap<Option<String>, Vec<Ptr<Workspace>>>,
    #[serde(serialize_with = "crate::snapshot::serialize_option_u64")]
    focused_workspace_id: Option<u64>,
    #[serde(serialize_with = "crate::snapshot::serialize_option_u64")]
    focused_window_id: Option<u64>,

    // not serialize fields
    #[serde(skip)]
    workspaces: HashMap<u64, Arc<Mutex<Workspace>>>,
    #[serde(skip)]
    windows: HashMap<u64, Arc<Mutex<Window>>>,
    #[serde(skip)]
    icon_cache: IconCache,
}

#[derive(serde::Serialize, Debug, Default)]
struct Workspace {
    id: u64,
    active: bool,
    urgent: bool,
    columns: Vec<Vec<Ptr<Window>>>,
    floatings: Vec<Ptr<Window>>,
    #[serde(serialize_with = "crate::snapshot::serialize_option_u64")]
    active_window_id: Option<u64>,

    // not serialize fields
    #[serde(skip)]
    idx: u8,
    #[serde(skip)]
    output: Option<String>,
}

#[derive(serde::Serialize, Clone, Debug, Default)]
struct Window {
    id: u64,
    #[serde(serialize_with = "crate::snapshot::serialize_option_string")]
    title: Option<String>,
    urgent: bool,
    #[serde(serialize_with = "crate::snapshot::serialize_option_string")]
    icon: Option<String>,

    // not serialize fields
    #[serde(skip)]
    workspace_id: Option<u64>,
    #[serde(skip)]
    layout: Option<(usize, usize)>,
    #[serde(skip)]
    app_id: Option<String>,
}

#[derive(Clone, Debug)]
struct Ptr<T>(Weak<Mutex<T>>);

impl<T: serde::Serialize> serde::Serialize for Ptr<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self.0.upgrade() {
            Some(c) => c.lock().unwrap().serialize(serializer),
            None => serializer.serialize_none(),
        }
    }
}

fn serialize_option_u64<S: serde::Serializer>(
    this: &Option<u64>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match this {
        Some(n) => serializer.serialize_u64(*n),
        None => serializer.serialize_u64(u64::MAX),
    }
}

fn serialize_option_string<S: serde::Serializer>(
    this: &Option<String>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match this {
        Some(n) => serializer.serialize_str(n),
        None => serializer.serialize_str(""),
    }
}

impl Snapshot {
    pub fn new(workspaces: Vec<niri_ipc::Workspace>, windows: Vec<niri_ipc::Window>) -> Self {
        let (workspaces, focused_workspace_id) = init_workspaces(&workspaces);
        let mut icon_cache = IconCache::new();
        let (windows, focused_window_id) = init_windows(&windows, &mut icon_cache);
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
        let stdout = io::stdout().lock();
        serde_json::to_writer(stdout, &self).unwrap();
        let mut stdout = io::stdout().lock();
        stdout.write(&[b'\n']).unwrap();
        stdout.flush().unwrap();
    }

    #[cfg(feature = "verify")]
    pub fn verify(&self, state: &niri_ipc::state::EventStreamState) {
        let mut sb = Vec::new();

        let workspaces = &state.workspaces.workspaces;
        if self.workspaces.len() != workspaces.len() {
            sb.push(format!(
                "Workspaces count defferent, local: {}, state: {}",
                self.workspaces.len(),
                workspaces.len()
            ));
        }
        let mut focused_workspace_id = None;
        for (id, workspace) in workspaces {
            let Some(ws) = self.workspaces.get(&id) else {
                sb.push(format!("Workspace not found in local: {id}"));
                continue;
            };
            let ws_ref = ws.lock().unwrap();
            if ws_ref.id != *id {
                sb.push(format!(
                    "Workspace id wrong, key: {id}, stored: {}",
                    ws_ref.id
                ));
            }
            if ws_ref.idx != workspace.idx {
                sb.push(format!(
                    "Workspace {id} idx different, local: {}, state: {}",
                    ws_ref.idx, workspace.idx
                ));
            }
            if ws_ref.output != workspace.output {
                sb.push(format!(
                    "Workspace {id} output different, local: {:?}, state: {:?}",
                    ws_ref.output, workspace.output
                ));
            }
            if ws_ref.active != workspace.is_active {
                sb.push(format!(
                    "Workspace {id} avtive different, local: {:?}, state: {:?}",
                    ws_ref.active, workspace.is_active
                ));
            }
            if ws_ref.urgent != workspace.is_urgent {
                sb.push(format!(
                    "Workspace {id} urgent different, local: {:?}, state: {:?}",
                    ws_ref.urgent, workspace.is_urgent
                ));
            }
            if workspace.is_focused {
                focused_workspace_id = Some(*id);
            }
        }
        if self.focused_workspace_id != focused_workspace_id {
            sb.push(format!(
                "Focused workspace different, local: {:?}, state: {:?}",
                self.focused_workspace_id, focused_workspace_id
            ));
        }

        let windows = &state.windows.windows;
        if self.windows.len() != windows.len() {
            sb.push(format!(
                "Windows count defferent, local: {}, state: {}",
                self.windows.len(),
                windows.len()
            ));
        }
        let mut focused_window_id = None;
        for (id, window) in windows {
            let Some(w) = self.windows.get(&id) else {
                sb.push(format!("Window not found in local: {id}"));
                continue;
            };
            let w_ref = w.lock().unwrap();
            if w_ref.id != *id {
                sb.push(format!("Window id wrong, key: {id}, stored: {}", w_ref.id));
            }
            if w_ref.workspace_id != window.workspace_id {
                sb.push(format!(
                    "Window {id} workspace_id different, local: {:?}, state: {:?}",
                    w_ref.workspace_id, window.workspace_id
                ));
            } else if let Some(workspace_id) = window.workspace_id {
                'ws: {
                    let Some(workspace) = self.workspaces.get(&workspace_id) else {
                        sb.push(format!("Window {id} workspace {workspace_id} not found"));
                        break 'ws;
                    };
                    let layout = window.layout.pos();
                    if w_ref.layout != layout {
                        sb.push(format!(
                            "Window {id} layout different, local: {:?}, state: {:?}",
                            w_ref.layout, layout
                        ));
                    } else if let Some((x, y)) = layout {
                        let workspace = workspace.lock().unwrap();
                        let Some(column) = workspace.columns.get(x) else {
                            sb.push(format!(
                                "Window {id} is at ({x}, {y}) but this column is not found"
                            ));
                            break 'ws;
                        };
                        let Some(tile) = column.get(y) else {
                            sb.push(format!(
                                "Window {id} is at ({x}, {y}) but this row is not found"
                            ));
                            break 'ws;
                        };
                        if Weak::as_ptr(&tile.0) != Arc::as_ptr(w) {
                            sb.push(format!(
                                "Window {id} is at ({x}, {y}) but the window here is {:?}",
                                tile.0.upgrade().map(|w| w.lock().unwrap().id)
                            ));
                        }
                    } else {
                        if !workspace
                            .lock()
                            .unwrap()
                            .floatings
                            .iter()
                            .any(|ptr| Weak::as_ptr(&ptr.0) == Arc::as_ptr(&w))
                        {
                            sb.push(format!(
                                "Window {id} is floating but not found in floatings"
                            ));
                        }
                    }
                }
            }

            if w_ref.title != window.title {
                sb.push(format!(
                    "Window {id} title different, local: {:?}, state: {:?}",
                    w_ref.title, window.title
                ));
            }
            if w_ref.app_id != window.app_id {
                sb.push(format!(
                    "Window {id} app_id different, local: {:?}, state: {:?}",
                    w_ref.app_id, window.app_id
                ));
            }
            if w_ref.urgent != window.is_urgent {
                sb.push(format!(
                    "Window {id} urgent different, local: {:?}, state: {:?}",
                    w_ref.urgent, window.is_urgent
                ));
            }
            if window.is_focused {
                focused_window_id = Some(*id);
            }
        }
        if self.focused_window_id != focused_window_id {
            sb.push(format!(
                "Focused window different, local: {:?}, state: {:?}",
                self.focused_window_id, focused_window_id
            ));
        }

        for s in sb {
            eprintln!("\x1B[31m{s}\x1B[0m");
        }
    }

    pub fn update(&mut self, evt: &niri_ipc::Event) -> Update {
        let result = match evt {
            niri_ipc::Event::WorkspacesChanged { workspaces } => {
                let (workspaces, focused_workspace_id) = init_workspaces(&workspaces);
                let outputs = init_outputs(&workspaces, &self.windows);
                self.outputs = outputs;
                self.focused_workspace_id = focused_workspace_id;
                self.workspaces = workspaces;
                Update::Consume
            }
            niri_ipc::Event::WorkspaceUrgencyChanged { id, urgent } => {
                match self.workspaces.get_mut(&id) {
                    Some(workspace) => {
                        workspace.lock().unwrap().urgent = *urgent;
                        Update::Consume
                    }
                    None => Update::Cache,
                }
            }
            niri_ipc::Event::WorkspaceActivated { id, focused } => {
                let Some(workspace) = self.workspaces.get(&id) else {
                    return Update::Cache;
                };
                workspace.lock().unwrap().active = true;
                let output_name = &workspace.lock().unwrap().output;
                let Some(output) = self.outputs.get_mut(output_name) else {
                    return Update::Cache;
                };
                for ws in output {
                    if let Some(ws) = ws.0.upgrade() {
                        if let Ok(ws) = &mut ws.try_lock() {
                            let ws_id = ws.id;
                            ws.active = ws_id == *id;
                        }
                    } else {
                        eprintln!(
                            "ERROR: Operating null workspace in output {:?}",
                            &output_name
                        );
                    }
                }
                if *focused {
                    self.focused_workspace_id = Some(*id);
                }
                Update::Consume
            }
            niri_ipc::Event::WorkspaceActiveWindowChanged {
                workspace_id,
                active_window_id,
            } => match self.workspaces.get_mut(&workspace_id) {
                Some(workspace) => {
                    if let Some(id) = active_window_id {
                        match self.windows.get(&id) {
                            Some(_) => {
                                workspace.lock().unwrap().active_window_id = *active_window_id;
                                Update::Consume
                            }
                            None => Update::Cache,
                        }
                    } else {
                        workspace.lock().unwrap().active_window_id = *active_window_id;
                        Update::Consume
                    }
                }
                None => Update::Cache,
            },
            niri_ipc::Event::WindowsChanged { windows: _ } => {
                // Ignored because this will only be sent on init
                Update::Ignore
            }
            niri_ipc::Event::WindowOpenedOrChanged { window } => {
                if let Some(found_window) = self.windows.get(&window.id) {
                    // Changed
                    // only for workspace_id / is_floating / title / app_id
                    // Or opened but was sent in wrong order
                    let found_window_ref = found_window.lock().unwrap();
                    let old_workspace_id = found_window_ref.workspace_id;
                    let workspace_changed = old_workspace_id != window.workspace_id;
                    let layout_changed = found_window_ref.layout != window.layout.pos();
                    let app_id_changed = found_window_ref.app_id != window.app_id;
                    let title_changed = found_window_ref.title != window.title;
                    drop(found_window_ref);
                    if !workspace_changed && !layout_changed && !title_changed && !app_id_changed {
                        return Update::Ignore;
                    }
                    if workspace_changed || layout_changed {
                        if let Some(workspace_id) = old_workspace_id {
                            if !self.workspaces.contains_key(&workspace_id) {
                                return Update::Cache;
                            }
                        }
                        if let Some(workspace_id) = window.workspace_id {
                            if !self.workspaces.contains_key(&workspace_id) {
                                return Update::Cache;
                            }
                        }

                        if let Some(workspace_id) = old_workspace_id {
                            remove_from_old_workspace(
                                &mut self.workspaces.get_mut(&workspace_id).unwrap(/*Already check up*/).lock().unwrap(),
                                &found_window.lock().unwrap(),
                            );
                        }
                        found_window.lock().unwrap().layout = window.layout.pos();
                        if let Some(workspace_id) = window.workspace_id {
                            add_to_workspace(
                                &mut self.workspaces.get_mut(&workspace_id).unwrap(/*Already check up*/).lock().unwrap(),
                                &found_window,
                            );
                        }
                    }
                    if title_changed || app_id_changed {
                        let mut found_window_ref = found_window.lock().unwrap();
                        found_window_ref.title = window.title.clone();
                        found_window_ref.icon = self.icon_cache.lookup(&window.app_id);
                        found_window_ref.app_id = window.app_id.clone();
                    }
                } else {
                    // Opened
                    if let Some(workspace_id) = window.workspace_id {
                        if !self.workspaces.contains_key(&workspace_id) {
                            return Update::Cache;
                        }
                    }
                    let window_ptr = Arc::new(Mutex::new(Window {
                        id: window.id,
                        title: window.title.clone(),
                        urgent: window.is_urgent,
                        icon: self.icon_cache.lookup(&window.app_id),
                        workspace_id: window.workspace_id,
                        layout: window.layout.pos(),
                        app_id: window.app_id.clone(),
                    }));
                    self.windows.insert(window.id, window_ptr.clone());
                    if let Some(workspace_id) = window.workspace_id {
                        add_to_workspace(
                            &mut self.workspaces.get_mut(&workspace_id).unwrap(/*Already check up*/).lock().unwrap(),
                            &window_ptr,
                        );
                    }
                };
                if window.is_focused {
                    self.focused_window_id = Some(window.id);
                }
                Update::Consume
            }
            niri_ipc::Event::WindowClosed { id } => {
                if let Some(window) = self.windows.remove(&id) {
                    let workspace_id = window.lock().unwrap().workspace_id;
                    if let Some(workspace_id) = workspace_id {
                        let workspace = self.workspaces.entry(workspace_id).or_default();
                        remove_from_old_workspace(
                            &mut workspace.lock().unwrap(),
                            &window.lock().unwrap(),
                        );
                    }
                    if self.focused_window_id == Some(*id) {
                        self.focused_window_id = None;
                    }
                    Update::Consume
                } else {
                    eprintln!("ERROR: Can't find window to be closed: {id}");
                    Update::Cache
                }
            }
            niri_ipc::Event::WindowFocusChanged { id } => {
                if let Some(id) = id {
                    match self.windows.get(id) {
                        Some(_) => {
                            self.focused_window_id = Some(*id);
                            Update::Consume
                        }
                        None => Update::Cache,
                    }
                } else {
                    self.focused_window_id = *id;
                    Update::Consume
                }
            }
            niri_ipc::Event::WindowUrgencyChanged { id, urgent } => {
                match self.windows.get_mut(&id) {
                    Some(window) => {
                        window.lock().unwrap().urgent = *urgent;
                        Update::Consume
                    }
                    None => Update::Cache,
                }
            }
            niri_ipc::Event::WindowLayoutsChanged { changes } => {
                let mut windows = Vec::with_capacity(changes.len());
                for change in changes {
                    let window = match self.windows.get(&change.0) {
                        Some(window) => window.clone(),
                        None => return Update::Cache,
                    };
                    if window.lock().unwrap().layout == change.1.pos() {
                        windows.push((None, None));
                        continue;
                    }
                    let workspace_id = window.lock().unwrap().workspace_id;
                    if let Some(workspace_id) = workspace_id {
                        match self.workspaces.get(&workspace_id) {
                            Some(workspace) => {
                                windows.push((Some(window), Some(workspace.clone())))
                            }
                            None => return Update::Cache,
                        }
                    } else {
                        windows.push((Some(window), None));
                    }
                }
                for ((window, workspace), change) in windows.iter().zip(changes) {
                    let Some(window) = window else {
                        continue;
                    };
                    if let Some(workspace) = workspace {
                        remove_from_old_workspace(
                            &mut workspace.lock().unwrap(),
                            &window.lock().unwrap(),
                        );
                    }
                    window.lock().unwrap().layout = change.1.pos();
                    if let Some(workspace) = workspace {
                        add_to_workspace(&mut workspace.lock().unwrap(), window);
                    }
                }
                Update::Consume
            }
            _ => Update::Ignore,
        };
        for workspace in self.workspaces.values_mut() {
            let mut workspace = workspace.lock().unwrap();
            workspace
                .columns
                .iter_mut()
                .for_each(|c| c.retain(|w| w.0.strong_count() != 0));
            workspace.columns.retain(|c| !c.is_empty());
            workspace.floatings.retain(|w| w.0.strong_count() != 0);
        }
        result
    }
}

trait LayoutExt {
    fn pos(&self) -> Option<(usize, usize)>;
}

impl LayoutExt for niri_ipc::WindowLayout {
    fn pos(&self) -> Option<(usize, usize)> {
        self.pos_in_scrolling_layout.map(|(x, y)| (x - 1, y - 1))
    }
}

/// Create workspace map from workspace vec sent by niri
fn init_workspaces(
    workspaces: &Vec<niri_ipc::Workspace>,
) -> (HashMap<u64, Arc<Mutex<Workspace>>>, Option<u64>) {
    let mut result = HashMap::new();
    let mut focused = None;
    for workspace in workspaces {
        if workspace.is_focused {
            focused = Some(workspace.id);
        }
        result.insert(
            workspace.id,
            Arc::new(Mutex::new(Workspace {
                id: workspace.id,
                active: workspace.is_active,
                urgent: workspace.is_urgent,
                columns: Vec::new(),
                floatings: Vec::new(),
                active_window_id: workspace.active_window_id,
                idx: workspace.idx,
                output: workspace.output.clone(),
            })),
        );
    }
    (result, focused)
}

/// Create windows map from windows vec sent by niri
fn init_windows(
    windows: &Vec<niri_ipc::Window>,
    icon_cache: &mut IconCache,
) -> (HashMap<u64, Arc<Mutex<Window>>>, Option<u64>) {
    let mut result = HashMap::new();
    let mut focused = None;
    for window in windows {
        if !window.is_floating && window.layout.pos_in_scrolling_layout.is_none() {
            eprintln!(
                "ERROR: Found window without pos nor floating {:?}",
                window.title
            );
            continue;
        }
        if window.is_focused {
            focused = Some(window.id);
        }
        result.insert(
            window.id,
            Arc::new(Mutex::new(Window {
                id: window.id,
                title: window.title.clone(),
                urgent: window.is_urgent,
                icon: icon_cache.lookup(&window.app_id),
                workspace_id: window.workspace_id,
                layout: window.layout.pos(),
                app_id: window.app_id.clone(),
            })),
        );
    }
    (result, focused)
}

/// Create outputs map from workspaces and windows map created or stored
fn init_outputs(
    workspaces: &HashMap<u64, Arc<Mutex<Workspace>>>,
    windows: &HashMap<u64, Arc<Mutex<Window>>>,
) -> HashMap<Option<String>, Vec<Ptr<Workspace>>> {
    let mut workspace_windows = HashMap::<_, (Vec<_>, Vec<Vec<_>>)>::new();
    for window in windows.values() {
        let Some(workspace_id) = window.lock().unwrap().workspace_id else {
            continue;
        };
        let (floatings, columns) = workspace_windows.entry(workspace_id).or_default();
        if let Some((x, y)) = window.lock().unwrap().layout {
            if columns.len() <= x {
                columns.resize(x + 1, Vec::new());
            }
            let column = &mut columns[x];
            if column.len() <= y {
                column.resize(y + 1, None);
            }
            column[y] = Some(Ptr(Arc::downgrade(&window)));
        } else {
            floatings.push(Some(Ptr(Arc::downgrade(&window))));
        }
    }

    let mut outputs = HashMap::<_, Vec<Ptr<Workspace>>>::new();
    for (id, workspace) in workspaces.iter() {
        outputs
            .entry(workspace.lock().unwrap().output.clone())
            .or_default()
            .push(Ptr(Arc::downgrade(&workspace)));
        if let Some((floatings, columns)) = workspace_windows.remove(id) {
            let mut workspace = workspace.lock().unwrap();
            workspace.columns = columns
                .into_iter()
                .map(|v| v.into_iter().filter_map(identity).collect())
                .collect();
            workspace.floatings = floatings.into_iter().filter_map(identity).collect();
        }
    }

    for output in outputs.values_mut() {
        output.sort_by_key(|workspace| match workspace.0.upgrade() {
            Some(workspace) => workspace.lock().unwrap().idx,
            None => u8::MAX,
        });
    }
    outputs
}

/// Mark a window removed from a workspace at its layout
///
/// [`remove`]: Vec::remove
fn remove_from_old_workspace(workspace: &mut Workspace, window: &Window) {
    if let Some((x, y)) = window.layout {
        let workspace_id = workspace.id;
        let columns = &mut workspace.columns;
        let Some(column) = columns.get_mut(x) else {
            eprintln!(
                "ERROR: Window {:?} column {} not found in workspace {}",
                window.title, x, workspace_id
            );
            return;
        };
        if column.len() <= y {
            eprintln!(
                "ERROR: Window {:?} row {} not found in column {}",
                window.title, y, x
            );
            return;
        }
        // There might be multiple removes and adds, and the window here is
        // possibly changed by an add, in which case we don't need to remove it
        if column[y].0.upgrade().is_some_and(|w| match w.try_lock() {
            Ok(w) => w.id == window.id,
            Err(_) => true,
        }) {
            column[y] = Ptr(Weak::new());
        }
    } else {
        let floatings = &mut workspace.floatings;
        floatings.iter_mut().for_each(|w| {
            if w.0.upgrade().is_some_and(|w| match w.try_lock() {
                Ok(w) => w.id == window.id,
                Err(_) => true,
            }) {
                *w = Ptr(Weak::new());
            }
        });
    }
}

/// Add a window to a workspace at a new layout with [`resize`] and [`index_mut`]
///
/// [`resize`]: Vec::resize
/// [`index_mut`]: std::ops::IndexMut::index_mut
fn add_to_workspace(workspace: &mut Workspace, window: &Arc<Mutex<Window>>) {
    window.lock().unwrap().workspace_id = Some(workspace.id);
    if let Some((x, y)) = window.lock().unwrap().layout {
        let columns = &mut workspace.columns;
        if columns.len() <= x {
            columns.resize(x + 1, Vec::new());
        }
        let column = &mut columns[x];
        if column.len() <= y {
            column.resize(y + 1, Ptr(Weak::new()));
        }
        column[y] = Ptr(Arc::downgrade(&window));
    } else {
        let floatings = &mut workspace.floatings;
        floatings.push(Ptr(Arc::downgrade(&window)));
    }
}
