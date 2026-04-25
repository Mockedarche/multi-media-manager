fn main() {
    #[cfg(target_os = "linux")]
    {
        // The previous fixes
        println!("cargo:rustc-link-lib=notify");
        println!("cargo:rustc-link-lib=gdk_pixbuf-2.0");

        // The new fix for SDL audio symbols
        println!("cargo:rustc-link-lib=SDL2");
    }
}
