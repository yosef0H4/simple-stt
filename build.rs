fn main() {
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=resources/windows.rc");
        println!("cargo:rerun-if-changed=resources/simple-stt.exe.manifest");
        embed_resource::compile("resources/windows.rc", embed_resource::NONE)
            .manifest_required()
            .expect("compile Windows resources");
    }
}
