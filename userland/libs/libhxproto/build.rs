fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("hamix") {
        println!("cargo:rustc-cdylib-link-arg=-soname=libhxproto.so.0");
    } else {
        println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libhxproto.so.0");
    }
}
