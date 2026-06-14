use std::{
    collections::{HashMap, HashSet}, fs::DirEntry, path::PathBuf, str::FromStr, sync::LazyLock
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

static EXTRA_ICON_DIRS: LazyLock<Vec<String>> = LazyLock::new(|| {
    let mut dirs = Vec::new();

    if let Ok(home) = std::env::var("HOME") {
        dirs.push(
            PathBuf::from(home)
                .join(".local/share/icons")
                .to_str()
                .unwrap()
                .to_string(),
        );
    }

    dirs
});

pub struct IconCache {
    known_icons: HashMap<String, Vec<String>>,
    known_sizes: Vec<u16>,
    map: HashMap<String, String>,
}

impl IconCache {
    pub fn new() -> Self {
        let known_icons = prepare_icon_names();
        let known_sizes = prepare_icon_sizes();
        Self {
            known_icons,
            known_sizes,
            map: HashMap::new(),
        }
    }

    pub fn lookup(&mut self, key: &Option<String>) -> Option<String> {
        let Some(key) = key else {
            return None;
        };
        let mut key = key.to_owned();
        simplify_key(&mut key);
        if let Some(path) = self.map.get(&key) {
            return Some(path.clone());
        }

        if let Some(icons) = self.known_icons.get(&key) {
            for icon in icons {
                if let Some(path) = self.lookup_icon(icon) {
                    self.map.insert(key.to_string(), path.clone());
                    return Some(path);
                }
            }
        } else if let Some(steam_id) = key.strip_prefix("steam_app_") {
            let mut icon = String::from_str("steam_icon_").unwrap();
            icon.push_str(steam_id);
            if let Some(path) = self.lookup_icon(&icon) {
                self.map.insert(key.to_string(), path.clone());
                return Some(path);
            }

            return None;
        }
        None
    }

    fn lookup_icon(&self, icon: &String) -> Option<String> {
        for size in &self.known_sizes {
            let mut paths = linicon::lookup_icon(&icon)
                .with_size(*size)
                .with_search_paths(&EXTRA_ICON_DIRS)
                .unwrap()
                .filter_map(Result::ok);
            while let Some(path) = paths.next() {
                if let Some(path) = path.path.to_str() {
                    let path = path.to_string();
                    return Some(path);
                }
            }
        }
        None
    }
}

fn simplify_key(key: &mut String) {
    key.make_ascii_lowercase();
}

fn prepare_icon_names() -> HashMap<String, Vec<String>> {
    let mut known_icons = HashMap::new();
    for dir in XDG_DATA_DIRS.iter() {
        let dir = dir.join("applications");
        if !dir.is_dir() || dir.is_symlink() {
            continue;
        }
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
            let Some(mut name) = name.to_str().map(str::to_string) else {
                return;
            };
            simplify_key(&mut name);
            known_icons.insert(name, icons.iter().map(String::clone).collect::<Vec<_>>());
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
