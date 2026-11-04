# gpui-kit 0.9.0, vendored

An unmodified copy of [gpui-kit](https://github.com/longbridge/gpui-kit) 0.9.0 from crates.io
(Apache-2.0), wired in with `[patch.crates-io]` in `../../Cargo.toml`, except for one change in
`Cargo.toml`: `default-features = false` on the `gpui_platform` (gpui-pre-platform) and `gpui_web`
(gpui-pre-web) dependencies.

Their default `multithreaded` feature builds `wasm_thread`, which uses `#![feature(...)]` and so
only compiles on nightly Rust. Multithreading in the browser also needs cross-origin isolation
(COOP/COEP headers), which GitHub Pages can't send. Native builds don't use the feature, so they are
unaffected.

Drop this copy and the patch once gpui-kit turns off those defaults itself, or when the playground
moves to a host that can serve COOP/COEP and threads are wanted.
