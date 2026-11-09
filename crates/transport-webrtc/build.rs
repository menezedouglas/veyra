fn main() {
    println!("cargo:rerun-if-env-changed=CMAKE_TOOLCHAIN_FILE");
    if std::env::var_os("CARGO_FEATURE_NATIVE_LIBNICE").is_some() {
        let path = std::env::var_os("CMAKE_TOOLCHAIN_FILE").expect("native-libnice requires CMAKE_TOOLCHAIN_FILE=<repo>/infrastructure/development/libdatachannel-nice.cmake; never build with libjuice silently");
        let source = std::fs::read_to_string(path)
            .expect("Cannot read the explicit libnice CMake toolchain");
        assert!(
            source.contains("set(USE_NICE ON"),
            "native-libnice requires USE_NICE=ON; libjuice cannot provide TURN/TCP"
        );
    }
}
