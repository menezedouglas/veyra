fn main() {
    if std::env::var_os("CARGO_FEATURE_NATIVE_LIBNICE").is_some()
        && std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux")
    {
        // Installed Debian sidecars and AppImage bin/ resolve bundled native libraries.
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/../lib/veyra:$ORIGIN/../lib");
    }
}
