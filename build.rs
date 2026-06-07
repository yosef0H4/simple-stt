fn main() {
    println!("cargo:rerun-if-changed=ui/settings.slint");
    slint_build::compile("ui/settings.slint").expect("compile Slint UI");
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rerun-if-changed=resources/windows.rc");
        println!("cargo:rerun-if-changed=resources/uvox.exe.manifest");
        embed_resource::compile("resources/windows.rc", embed_resource::NONE)
            .manifest_required()
            .expect("compile Windows resources");
    }
}
