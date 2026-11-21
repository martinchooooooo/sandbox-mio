#[path = "src/runtime/plugin_archive.rs"]
mod plugin_archive;

fn main() {
    // Release builds embed this archive; debug builds read `paseo-plugins/` at runtime instead
    // (`docs/features/paseo-plugins/02_dev.md`, The Concrete Model).
    println!("cargo:rerun-if-changed=../paseo-plugins");
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let (archive, ids) = plugin_archive::build_plugin_archive(std::path::Path::new("../paseo-plugins"))
        .expect("pack paseo-plugins/");
    std::fs::write(out_dir.join("paseo-plugins.tar"), archive).expect("write paseo-plugins.tar");
    std::fs::write(out_dir.join("paseo-plugin-ids.txt"), ids.join("\n")).expect("write paseo-plugin-ids.txt");
    tauri_build::build()
}
