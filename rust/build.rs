fn main() {
    slint_build::compile("ui/settings.slint").expect("compile Slint UI");
    #[cfg(target_os = "windows")]
    {
        println!(
            "cargo:rustc-link-arg-bin=uvox=/manifestdependency:type='win32' \
             name='Microsoft.Windows.Common-Controls' version='6.0.0.0' \
             processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
}
