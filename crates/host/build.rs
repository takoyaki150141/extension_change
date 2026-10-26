fn main() {
    println!("cargo:rerun-if-changed=resources.rc");
    println!("cargo:rerun-if-changed=ExtensionChange.exe.manifest");
    let _ = embed_resource::compile("resources.rc", embed_resource::NONE);
}
