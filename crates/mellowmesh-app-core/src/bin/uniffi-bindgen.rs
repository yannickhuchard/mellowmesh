//! UniFFI binding generator for `mellowmesh-app-core` (library mode).
//!
//! ```sh
//! cargo build -p mellowmesh-app-core
//! cargo run -p mellowmesh-app-core --features cli --bin uniffi-bindgen -- \
//!     generate --library target/debug/libmellowmesh_app_core.so \
//!     --language swift --out-dir out/swift
//! ```

fn main() {
    uniffi::uniffi_bindgen_main()
}
