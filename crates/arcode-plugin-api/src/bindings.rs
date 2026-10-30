//! Raw bindings generated from `wit/arcode-plugin.wit`.
//!
//! Plugin authors use the typed wrappers in the crate root instead; this
//! module stays public as an escape hatch and for the `export_plugin!` macro.
//! On non-wasm targets the generated imports are stubs that panic, which is
//! why the SDK routes every import through [`crate::sys`].

wit_bindgen::generate!({
    world: "plugin",
    path: "wit",
    pub_export_macro: true,
    export_macro_name: "export",
    default_bindings_module: "arcode_plugin::bindings",
    additional_derives: [PartialEq],
});
