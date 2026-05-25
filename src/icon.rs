use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    fs::DirEntry,
    path::PathBuf,
    sync::LazyLock,
};

const PREFER_SIZE: i32 = 16;

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

pub struct IconCache {
    map: HashMap<String, String>,
}

impl IconCache {
    pub fn new() -> Self {
        let known_icons = prepare_icon_names();
        let sizes = prepare_icon_sizes();
        let mut map = HashMap::new();
        for (name, icons) in known_icons {
            let Some(name) = name.to_str() else {
                continue;
            };
            let name = name.to_string();
            'icons: for icon in icons {
                for size in &sizes {
                    let mut paths = linicon::lookup_icon(&icon)
                        .with_size(*size)
                        .filter_map(Result::ok);
                    while let Some(path) = paths.next() {
                        if let Some(path) = path.path.to_str() {
                            map.insert(name, path.to_string());
                            map.insert(icon, path.to_string());
                            break 'icons;
                        }
                    }
                }
            }
        }
        Self { map }
    }

    pub fn lookup(&self, key: &Option<String>) -> Option<String> {
        if let Some(key) = key {
            if let Some(path) = self.map.get(key) {
                Some(path.clone())
            } else {
                None
            }
        } else {
            None
        }
    }
}

fn prepare_icon_names() -> HashMap<OsString, Vec<String>> {
    let mut known_icons = HashMap::new();
    for dir in XDG_DATA_DIRS.iter() {
        walk(&dir, &mut |entry| {
            if !entry.file_type().is_ok_and(|file_type| file_type.is_file()) {
                return;
            }
            let path = entry.path();
            let Some(name) = path.file_prefix() else {
                return;
            };
            let Some(ext) = path.extension() else {
                return;
            };
            if ext != "desktop" {
                return;
            }
            let entry = freedesktop_entry_parser::parse_entry(&path);
            let Ok(entry) = entry else {
                return;
            };
            let Some(icons) = entry.get("Desktop Entry", "Icon") else {
                return;
            };
            if icons.is_empty() {
                return;
            }
            known_icons.insert(
                name.to_owned(),
                icons.iter().map(String::clone).collect::<Vec<_>>(),
            );
        });
    }
    known_icons
}

fn prepare_icon_sizes() -> Vec<u16> {
    let mut sizes = HashSet::new();
    for dir in XDG_DATA_DIRS.iter() {
        let icon_dir = dir.join("icons");
        if icon_dir.is_dir() && !icon_dir.is_symlink() {
            let Ok(r) = icon_dir.read_dir() else {
                continue;
            };
            for theme_dir in r.filter_map(Result::ok) {
                let theme_dir = theme_dir.path();
                if theme_dir.is_dir() && !theme_dir.is_symlink() {
                    let Ok(r) = theme_dir.read_dir() else {
                        continue;
                    };
                    for entry in r.filter_map(Result::ok) {
                        let path = entry.path();
                        let Some(name) = path.file_name() else {
                            continue;
                        };
                        let Some(name) = name.to_str() else {
                            continue;
                        };
                        let name = name.to_string();
                        let Some((size, _)) = name.split_once('x') else {
                            continue;
                        };
                        if let Ok(size) = size.parse() {
                            sizes.insert(size);
                        }
                    }
                }
            }
        }
    }
    let mut sizes = sizes.into_iter().collect::<Vec<_>>();
    sizes.sort_by(|l, r| {
        let l = *l as i32;
        let r = *r as i32;
        let dl = (l - PREFER_SIZE).abs();
        let dr = (r - PREFER_SIZE).abs();
        if dl == dr { r.cmp(&l) } else { dl.cmp(&dr) }
    });
    sizes
}

fn walk<F: FnMut(&DirEntry)>(dir: &PathBuf, cb: &mut F) {
    if dir.is_dir() && !dir.is_symlink() {
        let Ok(r) = dir.read_dir() else {
            return;
        };
        for entry in r.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() && !path.is_symlink() {
                walk(&path, cb);
            } else {
                cb(&entry);
            }
        }
    }
}
