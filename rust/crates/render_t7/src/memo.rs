//! Things made once per device and kept: compiled programs and pipelines.
//! A caller that asks for a key another thread is making waits for it, so
//! the load can start making them early (`prewarm`) and the stage that needs
//! them finds them made.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

struct Entry {
    device: wgpu::Device,
    cell: OnceLock<Box<dyn Any + Send + Sync>>,
}

/// The value kept under `key` for `device`, made by `build` the first time.
pub fn get<T: Clone + Send + Sync + 'static>(device: &wgpu::Device, key: String, build: impl FnOnce() -> T) -> T {
    static MEMO: OnceLock<Mutex<HashMap<String, Arc<Entry>>>> = OnceLock::new();
    let entry = {
        let Ok(mut m) = MEMO.get_or_init(Default::default).lock() else { return build() };
        match m.get(&key) {
            Some(e) if e.device == *device => e.clone(),
            _ => {
                let e = Arc::new(Entry { device: device.clone(), cell: OnceLock::new() });
                m.insert(key, e.clone());
                e
            }
        }
    };
    entry.cell.get_or_init(|| Box::new(build())).downcast_ref::<T>().expect("memo key reused for another type").clone()
}
