//! bo2mp: Black Ops III's compiled scripts: the code is `gsc_t6::t7`
//! (the VM's crate, so the sim needs no new dependency); this crate holds
//! its table generator (`gen/t7gen.py`), the tests over the retail zones
//! and the tools (`t7_dis`).
#![forbid(unsafe_code)]

pub use gsc_t6::t7::*;
