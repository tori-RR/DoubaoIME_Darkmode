fn main() {
    // tauri-build embeds icons/icon.ico into the PE via winres, but without
    // the codegen feature it only watches tauri.conf.json. Icon-only edits
    // would otherwise keep linking a stale resource.lib (Explorer file icon).
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=icons/32x32.png");
    println!("cargo:rerun-if-changed=icons/128x128.png");
    println!("cargo:rerun-if-changed=icons/128x128@2x.png");
    tauri_build::build()
}
