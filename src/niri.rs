use std::{cell::RefCell, rc::{Rc, Weak}};

#[derive(serde::Serialize, Debug, Default)]
pub struct Workspace {
    pub id: u64,
    pub active: bool,
    pub urgent: bool,
    pub columns: Vec<Vec<Ptr<Window>>>,
    pub floatings: Vec<Ptr<Window>>,
    #[serde(serialize_with = "crate::niri::serialize_option_u64")]
    pub active_window_id: Option<u64>,

    // not serialize fields
    #[serde(skip)]
    pub idx: u8,
    #[serde(skip)]
    pub output: Option<String>,
}

#[derive(serde::Serialize, Debug, Default)]
pub struct Window {
    pub id: u64,
    #[serde(serialize_with = "crate::niri::serialize_option_string")]
    pub title: Option<String>,
    pub urgent: bool,
    #[serde(serialize_with = "crate::niri::serialize_option_string")]
    pub icon: Option<String>,

    // not serialize fields
    #[serde(skip)]
    pub workspace_id: Option<u64>,
    #[serde(skip)]
    pub layout: WindowLayout,
    #[serde(skip)]
    pub app_id: Option<String>,
}

impl Workspace {
    /// Mark a window removed from a workspace at its layout with [`index_mut`]
    /// Assumes all windows in the same column of the layout are not borrowed in current thread
    ///
    /// [`index_mut`]: std::ops::IndexMut::index_mut
    pub fn remove(&mut self, layout: WindowLayout, id: u64) {
        if let WindowLayout::Fix { x, y } = layout {
            let workspace_id = self.id;
            let columns = &mut self.columns;
            let Some(column) = columns.get_mut(x) else {
                eprintln!(
                    "ERROR: Window {} at column {} but is not found in workspace {}",
                    id, x, workspace_id
                );
                return;
            };
            if column.len() <= y {
                eprintln!(
                    "ERROR: Window {} at row {} but is not found in column {}",
                    id, y, x
                );
                return;
            }
            if column[y].0.upgrade().is_some_and(|w| w.borrow().id == id) {
                column[y] = Ptr(Weak::new());
            }
        } else {
            let floatings = &mut self.floatings;
            floatings.retain(|w| w.0.upgrade().is_some_and(|w| w.borrow().id == id));
        }
    }

    /// Add a window to a workspace at a new layout with [`resize`] and [`index_mut`]
    /// Assumes this window is not borrowed in current thread
    ///
    /// [`resize`]: Vec::resize
    /// [`index_mut`]: std::ops::IndexMut::index_mut
    pub fn add(&mut self, window: &Rc<RefCell<Window>>) {
        window.borrow_mut().workspace_id = Some(self.id);
        if let WindowLayout::Fix { x, y } = window.borrow().layout {
            let columns = &mut self.columns;
            if columns.len() <= x {
                columns.resize(x + 1, Vec::new());
            }
            let column = &mut columns[x];
            if column.len() <= y {
                column.resize(y + 1, Ptr(Weak::new()));
            }
            column[y] = Ptr(Rc::downgrade(&window));
        } else {
            let floatings = &mut self.floatings;
            floatings.push(Ptr(Rc::downgrade(&window)));
        }
    }
}

#[derive(Debug)]
pub struct Ptr<T>(pub Weak<RefCell<T>>);

impl<T> Clone for Ptr<T> {
    fn clone(&self) -> Self {
        Ptr(Weak::clone(&self.0))
    }
}

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

pub fn serialize_option_u64<S: serde::Serializer>(
    this: &Option<u64>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match this {
        Some(n) => serializer.serialize_u64(*n),
        None => serializer.serialize_u64(u64::MAX),
    }
}

pub fn serialize_option_string<S: serde::Serializer>(
    this: &Option<String>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match this {
        Some(n) => serializer.serialize_str(n),
        None => serializer.serialize_str(""),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WindowLayout {
    Fix {
        x: usize,
        y: usize,
    },
    #[default]
    Float,
}

pub trait IntoWindowLayout {
    fn layout(&self) -> WindowLayout;
}

impl IntoWindowLayout for niri_ipc::WindowLayout {
    fn layout(&self) -> WindowLayout {
        match self.pos_in_scrolling_layout {
            Some((x, y)) => WindowLayout::Fix { x: x - 1, y: y - 1 },
            None => WindowLayout::Float,
        }
    }
}
