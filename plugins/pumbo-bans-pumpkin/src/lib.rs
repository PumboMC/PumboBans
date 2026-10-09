//! PumboBans for the Pumpkin server.
//!
//! All rules live in `pumbo-bans-core`; this crate translates Pumpkin events
//! (pre-login, join, chat, commands, inter-plugin messages) into calls of the
//! core and carries out what it returns. [`setup`] holds the parts that do not
//! need the host (config template, messages, permission nodes on Pumpkin) and is
//! tested natively; `plugin` is the WebAssembly glue.

pub mod setup;

#[cfg(all(
    target_arch = "wasm32",
    any(
        all(feature = "mc263", feature = "mc262"),
        all(feature = "mc263", feature = "mcgit"),
        all(feature = "mc262", feature = "mcgit")
    )
))]
compile_error!("enable exactly one of the features `mc263`, `mc262` and `mcgit`");
#[cfg(all(target_arch = "wasm32", not(any(feature = "mc263", feature = "mc262", feature = "mcgit"))))]
compile_error!(
    "enable one of the features `mc263` (Pumpkin 0.2.0), `mc262` (Pumpkin 0.1.0-dev) or `mcgit` (Pumpkin with the Pumbo hooks)"
);

#[cfg(all(target_arch = "wasm32", feature = "mc262", not(feature = "mc263"), not(feature = "mcgit")))]
extern crate api262 as papi;
#[cfg(all(target_arch = "wasm32", feature = "mc263", not(feature = "mc262"), not(feature = "mcgit")))]
extern crate api263 as papi;
#[cfg(all(target_arch = "wasm32", feature = "mcgit", not(feature = "mc263"), not(feature = "mc262")))]
extern crate apigit as papi;

#[cfg(all(target_arch = "wasm32", any(feature = "mc263", feature = "mc262", feature = "mcgit")))]
mod plugin;
