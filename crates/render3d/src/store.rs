//! Where the viewer keeps its saved game between runs: small named text files. On the desktop they are files in
//! the user's settings folder (`$XDG_CONFIG_HOME/rts-3d`, else `~/.config/rts-3d`, else `%APPDATA%\rts-3d` on
//! Windows); in the browser they are the page's local storage, under `rts-3d/`. Tests use a folder of their own, or
//! memory. The Classic engine keeps its saves the same way (rts-engine `classic-render::store`).

use std::collections::BTreeMap;
use std::path::PathBuf;

pub enum Store {
    /// Files in a folder, made the first time something is written.
    Dir(PathBuf),
    /// The browser page's local storage.
    #[cfg(target_arch = "wasm32")]
    Browser,
    /// Nothing kept past the run, for tests and when no folder can be found.
    Memory(BTreeMap<String, String>),
}

impl Store {
    /// The player's own store on this machine or in this browser.
    pub fn user() -> Store {
        #[cfg(target_arch = "wasm32")]
        return Store::Browser;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
            let base =
                var("XDG_CONFIG_HOME").or_else(|| var("HOME").map(|h| h.join(".config"))).or_else(|| var("APPDATA"));
            match base {
                Some(b) => Store::Dir(b.join("rts-3d")),
                None => Store::Memory(BTreeMap::new()),
            }
        }
    }

    /// The text stored under `name`, if there is any.
    pub fn read(&self, name: &str) -> Option<String> {
        match self {
            Store::Dir(d) => std::fs::read_to_string(d.join(name)).ok(),
            #[cfg(target_arch = "wasm32")]
            Store::Browser => local_storage()?.get_item(&format!("rts-3d/{name}")).ok().flatten(),
            Store::Memory(m) => m.get(name).cloned(),
        }
    }

    /// Keep `text` under `name`, replacing what was there.
    pub fn write(&mut self, name: &str, text: &str) -> Result<(), String> {
        match self {
            Store::Dir(d) => {
                std::fs::create_dir_all(&*d).map_err(|e| format!("{}: {e}", d.display()))?;
                let path = d.join(name);
                std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
            }
            #[cfg(target_arch = "wasm32")]
            Store::Browser => local_storage()
                .ok_or("this browser keeps nothing for the page")?
                .set_item(&format!("rts-3d/{name}"), text)
                .map_err(|_| "the browser refused to keep it (is its storage full?)".to_string()),
            Store::Memory(m) => {
                m.insert(name.to_string(), text.to_string());
                Ok(())
            }
        }
    }

    /// Where things are kept, for a message.
    pub fn describe(&self) -> String {
        match self {
            Store::Dir(d) => d.display().to_string(),
            #[cfg(target_arch = "wasm32")]
            Store::Browser => "the browser's storage for this page".to_string(),
            Store::Memory(_) => "memory only".to_string(),
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}
