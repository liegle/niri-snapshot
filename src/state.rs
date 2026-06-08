use std::{
    cell::RefCell,
    collections::HashMap,
    convert::identity,
    rc::Rc,
};

use crate::{
    icon::IconCache,
    niri::{IntoWindowLayout, Ptr, Window, WindowLayout, Workspace},
    snapshot::Update,
};

#[derive(serde::Serialize)]
pub struct State {
    outputs: HashMap<Option<String>, Vec<Ptr<Workspace>>>,
    #[serde(serialize_with = "crate::niri::serialize_option_u64")]
    focused_workspace_id: Option<u64>,
    #[serde(serialize_with = "crate::niri::serialize_option_u64")]
    focused_window_id: Option<u64>,

    // not serialize fields
    #[serde(skip)]
    workspaces: HashMap<u64, Rc<RefCell<Workspace>>>,
    #[serde(skip)]
    windows: HashMap<u64, Rc<RefCell<Window>>>,
    #[serde(skip)]
    icon_cache: IconCache,
}

impl State {
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

    #[cfg(feature = "verify")]
    pub fn verify(&self, state: &niri_ipc::state::EventStreamState) {
        use std::rc::Weak;

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
            let ws_ref = ws.borrow();
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
            let w_ref = w.borrow();
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
                    let layout = window.layout.layout();
                    if w_ref.layout != layout {
                        sb.push(format!(
                            "Window {id} layout different, local: {:?}, state: {:?}",
                            w_ref.layout, layout
                        ));
                    } else if let WindowLayout::Fix { x, y } = layout {
                        let workspace = workspace.borrow();
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
                        if Weak::as_ptr(&tile.0) != Rc::as_ptr(w) {
                            sb.push(format!(
                                "Window {id} is at ({x}, {y}) but the window here is {:?}",
                                tile.0.upgrade().map(|w| w.borrow().id)
                            ));
                        }
                    } else {
                        if !workspace
                            .borrow()
                            .floatings
                            .iter()
                            .any(|ptr| Weak::as_ptr(&ptr.0) == Rc::as_ptr(&w))
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
                workspace.borrow_mut().active = true;
                let output_name = workspace.borrow().output.clone();
                let Some(output) = self.outputs.get_mut(&output_name) else {
                    return Update::Cache;
                };
                for ws in output {
                    if let Some(ws) = ws.0.upgrade() {
                        let ws_id = ws.borrow().id;
                        ws.borrow_mut().active = ws_id == *id;
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
                    let found_window_ref = found_window.borrow();
                    let old_workspace_id = found_window_ref.workspace_id;
                    let workspace_changed = old_workspace_id != window.workspace_id;
                    let old_layout = found_window_ref.layout;
                    let layout_changed = old_layout != window.layout.layout();
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
                            self.workspaces
                                .get_mut(&workspace_id)
                                .unwrap(/*Already check above*/)
                                .borrow_mut()
                                .remove(old_layout, window.id);
                        }
                        found_window.borrow_mut().layout = window.layout.layout();
                        if let Some(workspace_id) = window.workspace_id {
                            self.workspaces
                                .get_mut(&workspace_id)
                                .unwrap(/*Already check above*/)
                                .borrow_mut()
                                .add(&found_window);
                        }
                    }
                    if title_changed || app_id_changed {
                        let mut found_window_ref = found_window.borrow_mut();
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
                    let window_ptr = Rc::new(RefCell::new(Window {
                        id: window.id,
                        title: window.title.clone(),
                        urgent: window.is_urgent,
                        icon: self.icon_cache.lookup(&window.app_id),
                        workspace_id: window.workspace_id,
                        layout: window.layout.layout(),
                        app_id: window.app_id.clone(),
                    }));
                    self.windows.insert(window.id, window_ptr.clone());
                    if let Some(workspace_id) = window.workspace_id {
                        self.workspaces
                            .get_mut(&workspace_id)
                            .unwrap(/*Already check above*/)
                            .borrow_mut()
                            .add(&window_ptr);
                    }
                };
                if window.is_focused {
                    self.focused_window_id = Some(window.id);
                }
                Update::Consume
            }
            niri_ipc::Event::WindowClosed { id } => {
                if let Some(window) = self.windows.remove(&id) {
                    let workspace_id = window.borrow().workspace_id;
                    let layout = window.borrow().layout;
                    if let Some(workspace_id) = workspace_id {
                        let workspace = self.workspaces.entry(workspace_id).or_default();
                        workspace.borrow_mut().remove(layout, *id);
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
                    if window.borrow().layout == change.1.layout() {
                        windows.push((None, None));
                        continue;
                    }
                    let workspace_id = window.borrow().workspace_id;
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
                        let layout = window.borrow().layout;
                        let id = window.borrow().id;
                        workspace.borrow_mut().remove(layout, id);
                    }
                    window.borrow_mut().layout = change.1.layout();
                    if let Some(workspace) = workspace {
                        workspace.borrow_mut().add(window);
                    }
                }
                Update::Consume
            }
            _ => Update::Ignore,
        };
        for workspace in self.workspaces.values_mut() {
            let mut workspace = workspace.borrow_mut();
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
            Rc::new(RefCell::new(Window {
                id: window.id,
                title: window.title.clone(),
                urgent: window.is_urgent,
                icon: icon_cache.lookup(&window.app_id),
                workspace_id: window.workspace_id,
                layout: window.layout.layout(),
                app_id: window.app_id.clone(),
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
        let (floatings, columns) = workspace_windows.entry(workspace_id).or_default();
        if let WindowLayout::Fix { x, y } = window.borrow().layout {
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
