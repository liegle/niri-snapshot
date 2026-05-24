use std::{
    cell::RefCell,
    collections::HashMap,
    convert::identity,
    rc::{Rc, Weak},
};

use crate::{Update, icon::IconCache};

#[derive(serde::Serialize)]
pub struct Snapshot {
    outputs: HashMap<Option<String>, Vec<Ptr<Workspace>>>,
    focused_workspace_id: Option<u64>,
    focused_window_id: Option<u64>,

    // not serialize fields
    #[serde(skip)]
    workspaces: HashMap<u64, Rc<RefCell<Workspace>>>,
    #[serde(skip)]
    windows: HashMap<u64, Rc<RefCell<Window>>>,
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
    title: Option<String>,
    urgent: bool,
    icon: Option<String>,

    // not serialize fields
    #[serde(skip)]
    workspace_id: Option<u64>,
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
        println!("{}", serde_json::to_string(&self).unwrap());
    }

    #[cfg(test)]
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
            let ws = ws.borrow();
            if ws.id != *id {
                sb.push(format!("Workspace id wrong, key: {id}, stored: {}", ws.id));
            }
            if ws.idx != workspace.idx {
                sb.push(format!(
                    "Workspace {id} idx different, local: {}, state: {}",
                    ws.idx, workspace.idx
                ));
            }
            if ws.output != workspace.output {
                sb.push(format!(
                    "Workspace {id} output different, local: {:?}, state: {:?}",
                    ws.output, workspace.output
                ));
            }
            if ws.active != workspace.is_active {
                sb.push(format!(
                    "Workspace {id} avtive different, local: {:?}, state: {:?}",
                    ws.active, workspace.is_active
                ));
            }
            if ws.urgent != workspace.is_urgent {
                sb.push(format!(
                    "Workspace {id} urgent different, local: {:?}, state: {:?}",
                    ws.urgent, workspace.is_urgent
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
            let w = w.borrow();
            if w.id != *id {
                sb.push(format!("Window id wrong, key: {id}, stored: {}", w.id));
            }
            // if let Some(workspace_id) = window.workspace_id {
            //     if w.workspace_id != workspace_id {
            //         sb.push(format!(
            //             "Window {id} workspace_id different, local: {}, state: {}",
            //             w.workspace_id, workspace_id
            //         ));
            //     }
            // } else {
            //     sb.push(format!("Window {id} workspace id different, local: {}, state: None", w.workspace_id));
            // };
            if w.title != window.title {
                sb.push(format!(
                    "Window {id} title different, local: {:?}, state: {:?}",
                    w.title, window.title
                ));
            }
            let icon = self.icon_cache.lookup_no_insert(&window.app_id);
            if w.icon != icon {
                sb.push(format!(
                    "Window {id} icon different, local: {:?}, state: {:?}",
                    w.icon, icon
                ));
            }
            if w.urgent != window.is_urgent {
                sb.push(format!(
                    "Window {id} urgent different, local: {:?}, state: {:?}",
                    w.urgent, window.is_urgent
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
    }

    pub fn update(&mut self, evt: &niri_ipc::Event) -> Update {
        match evt {
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
                        workspace.borrow_mut().urgent = *urgent;
                        Update::Consume
                    }
                    None => Update::Cache,
                }
            }
            niri_ipc::Event::WorkspaceActivated { id, focused } => {
                let Some(workspace) = self.workspaces.get(&id) else {
                    return Update::Cache;
                };
                let output_name = &workspace.borrow().output;
                let Some(output) = self.outputs.get_mut(output_name) else {
                    return Update::Cache;
                };
                for ws in output {
                    if let Some(ws) = ws.0.upgrade() {
                        let ws_id = ws.borrow().id;
                        if let Ok(ws) = &mut ws.try_borrow_mut() {
                            ws.active = ws_id == *id;
                        }
                    } else {
                        eprintln!("Operating null workspace in output {:?}", &output_name);
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
                                workspace.borrow_mut().active_window_id = *active_window_id;
                                Update::Consume
                            }
                            None => Update::Cache,
                        }
                    } else {
                        workspace.borrow_mut().active_window_id = *active_window_id;
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
                    let mut found_window_ref = found_window.borrow_mut();
                    if found_window_ref.workspace_id != window.workspace_id
                        || found_window_ref.layout != window.layout.pos()
                    {
                        if let Some(workspace_id) = found_window_ref.workspace_id {
                            if !self.workspaces.contains_key(&workspace_id) {
                                return Update::Cache;
                            }
                        }
                        if let Some(workspace_id) = window.workspace_id {
                            if !self.workspaces.contains_key(&workspace_id) {
                                return Update::Cache;
                            }
                        }

                        if let Some(workspace_id) = found_window_ref.workspace_id {
                            remove_from_old_workspace(
                                &mut self.workspaces.get_mut(&workspace_id).unwrap(/*Already check up*/).borrow_mut(),
                                &found_window_ref,
                            );
                        }
                        found_window_ref.layout = window.layout.pos();
                        if let Some(workspace_id) = window.workspace_id {
                            add_to_workspace(
                                &mut self.workspaces.get_mut(&workspace_id).unwrap(/*Already check up*/).borrow_mut(),
                                &found_window,
                            );
                        }
                    }
                    found_window_ref.title = window.title.clone();
                    found_window_ref.icon = self.icon_cache.lookup(&window.app_id);
                } else {
                    // Opened
                    if let Some(workspace_id) = window.workspace_id {
                        if !self.workspaces.contains_key(&workspace_id) {
                            return Update::Cache;
                        }
                    }
                    let window_ptr = Rc::new(RefCell::new(Window {
                        id: window.id,
                        title: window.title.clone(),
                        urgent: window.is_urgent,
                        icon: self.icon_cache.lookup(&window.app_id),
                        workspace_id: window.workspace_id,
                        layout: window.layout.pos(),
                    }));
                    self.windows.insert(window.id, window_ptr.clone());
                    if let Some(workspace_id) = window.workspace_id {
                        add_to_workspace(
                            &mut self.workspaces.get_mut(&workspace_id).unwrap(/*Already check up*/).borrow_mut(),
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
                    if let Some(workspace_id) = window.borrow().workspace_id {
                        let workspace = self.workspaces.entry(workspace_id).or_default();
                        remove_from_old_workspace(&mut workspace.borrow_mut(), &window.borrow());
                    }
                    Update::Consume
                } else {
                    eprintln!("Can't find window to be closed: {id}");
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
                    self.focused_workspace_id = *id;
                    Update::Consume
                }
            }
            niri_ipc::Event::WindowUrgencyChanged { id, urgent } => {
                match self.windows.get_mut(&id) {
                    Some(window) => {
                        window.borrow_mut().urgent = *urgent;
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
                    let workspace_id = window.borrow().workspace_id;
                    if let Some(workspace_id) = workspace_id {
                        match self.workspaces.get(&workspace_id) {
                            Some(workspace) => windows.push((window, Some(workspace.clone()))),
                            None => return Update::Cache,
                        }
                    } else {
                        windows.push((window, None));
                    }
                }
                for ((window, workspace), change) in windows.iter().zip(changes) {
                    if let Some(workspace) = workspace {
                        remove_from_old_workspace(&mut workspace.borrow_mut(), &window.borrow());
                    }
                    window.borrow_mut().layout = change.1.pos();
                    if let Some(workspace) = workspace {
                        add_to_workspace(&mut workspace.borrow_mut(), window);
                    }
                }
                Update::Consume
            }
            _ => Update::Ignore,
        }
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
) -> (HashMap<u64, Rc<RefCell<Window>>>, Option<u64>) {
    let mut result = HashMap::new();
    let mut focused = None;
    for window in windows {
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
                title: window.title.clone(),
                urgent: window.is_urgent,
                icon: icon_cache.lookup(&window.app_id),
                workspace_id: window.workspace_id,
                layout: window.layout.pos(),
            })),
        );
    }
    (result, focused)
}

/// Create outputs map from workspaces and windows map created or stored
fn init_outputs(
    workspaces: &HashMap<u64, Rc<RefCell<Workspace>>>,
    windows: &HashMap<u64, Rc<RefCell<Window>>>,
) -> HashMap<Option<String>, Vec<Ptr<Workspace>>> {
    let mut workspace_windows = HashMap::<_, (Vec<_>, Vec<Vec<_>>)>::new();
    for window in windows.values() {
        let Some(workspace_id) = window.borrow().workspace_id else {
            continue;
        };
        let (floatings, columns) = workspace_windows
            .entry(workspace_id)
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

    let mut outputs = HashMap::<_, Vec<Ptr<Workspace>>>::new();
    for (id, workspace) in workspaces.iter() {
        outputs
            .entry(workspace.borrow().output.clone())
            .or_default()
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
        output.sort_by_key(|workspace| match workspace.0.upgrade() {
            Some(workspace) => workspace.borrow().idx,
            None => u8::MAX,
        });
    }
    outputs
}

/// Remove window from a workspace at its layout with [`remove`]
///
/// [`remove`]: Vec::remove
fn remove_from_old_workspace(workspace: &mut Workspace, window: &Window) {
    if let Some((x, y)) = window.layout {
        let workspace_id = workspace.id;
        let columns = &mut workspace.columns;
        let Some(column) = columns.get_mut(x) else {
            eprintln!(
                "Window {:?} column {} not found in workspace {}",
                window.title, x, workspace_id
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
        let floatings = &mut workspace.floatings;
        floatings.retain(|w| w.0.upgrade().is_some_and(|w| w.borrow().id != window.id));
    }
}

/// Add a window to a workspace at a new layout with [`resize`] and [`index_mut`]
///
/// [`resize`]: Vec::resize
/// [`index_mut`]: std::ops::IndexMut::index_mut
fn add_to_workspace(workspace: &mut Workspace, window: &Rc<RefCell<Window>>) {
    if let Some((x, y)) = window.borrow().layout {
        let columns = &mut workspace.columns;
        if columns.len() <= x {
            columns.resize(x + 1, Vec::new());
        }
        let column = &mut columns[x];
        if column.len() <= y {
            column.resize(y + 1, Ptr(Weak::new()));
        }
        column[y] = Ptr(Rc::downgrade(&window));
    } else {
        let floatings = &mut workspace.floatings;
        floatings.push(Ptr(Rc::downgrade(&window)));
    }
}
