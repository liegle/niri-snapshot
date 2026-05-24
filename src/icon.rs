// copied from niri-taskbar
// MIT License
// Copyright (c) 2024 Adam Harvey
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use std::{collections::HashMap, path::PathBuf, str::FromStr, sync::LazyLock};

use gtk::gio::{
    DesktopAppInfo,
    traits::{AppInfoExt as _, IconExt},
};

pub struct IconCache {
    map: HashMap<String, String>,
}

impl IconCache {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn lookup(&mut self, key: &Option<String>) -> Option<String> {
        let Some(key) = key else {
            return None;
        };

        if let Some(path) = self.map.get(key) {
            return Some(path.clone());
        }

        if let Some(path) = lookup(key) {
            self.map.insert(key.clone(), path.clone());
            return Some(path);
        }

        None
    }

    #[cfg(feature = "verify")]
    pub fn lookup_no_insert(&self, key: &Option<String>) -> Option<String> {
        let Some(key) = key else {
            return None;
        };

        if let Some(path) = self.map.get(key) {
            return Some(path.clone());
        }

        if let Some(path) = lookup(key) {
            return Some(path);
        }

        None
    }
}

static XDG_DATA_DIRS: LazyLock<Vec<PathBuf>> = LazyLock::new(|| {
    let mut dirs = Vec::new();

    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share"));
    }

    if let Ok(env) = std::env::var("XDG_DATA_DIRS") {
        dirs.extend(env.split(':').map(PathBuf::from))
    } else {
        dirs.extend(
            ["/usr/share", "/usr/local/share"]
                .into_iter()
                .map(PathBuf::from),
        );
    }

    dirs
});

fn lookup(key: &str) -> Option<String> {
    // KDE applications are special, so we'll go hunt for them ourselves. Again, this is loosely
    // adapted from wlr/taskbar.
    for dir in XDG_DATA_DIRS.iter() {
        for prefix in [
            "applications/",
            "applications/kde/",
            "applications/org.kde.",
        ] {
            for suffix in ["", ".desktop"] {
                let path = dir.join(format!("{prefix}{key}{suffix}"));
                if let Some(info) = DesktopAppInfo::from_filename(&path) {
                    if let Some(path) = info.icon_path() {
                        return Some(path);
                    }
                }
            }
        }
    }

    // This is _very_ roughly adapted from the wlr/taskbar module built into Waybar. We don't do
    // the same startup_wm_class check here for now.
    let infos = DesktopAppInfo::search(key);
    for possible in infos.into_iter().flatten() {
        if let Some(info) = DesktopAppInfo::new(&possible) {
            if let Some(path) = info.icon_path() {
                return Some(path);
            }
        }
    }

    None
}

fn lookup_icon(key: &str) -> Option<String> {
    if let Some(path) = freedesktop_icons::lookup(key).with_size(16).find() {
        return convert(path);
    }

    if let Some(path) = linicon::lookup_icon(key)
        .with_size(16)
        .filter_map(|result| result.ok())
        .next()
    {
        return convert(path.path);
    }

    None
}

fn convert(path: PathBuf) -> Option<String> {
    if let Some(path) = path.to_str() {
        match String::from_str(path) {
            Ok(path) => return Some(path),
            _ => (),
        }
    }
    eprint!("Can't convert path into string");
    return None;
}

trait DesktopAppInfoExt {
    fn icon_path(&self) -> Option<String>;
}

impl DesktopAppInfoExt for DesktopAppInfo {
    fn icon_path(&self) -> Option<String> {
        self.icon()
            .and_then(|icon| IconExt::to_string(&icon))
            .and_then(|name| lookup_icon(&name))
    }
}
