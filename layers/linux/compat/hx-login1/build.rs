fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("hamix") {
        return;
    }
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rerun-if-changed=../../../../userland/link.ld");
    println!("cargo:rustc-link-arg=-T{}/../../../../userland/link.ld", manifest_dir);
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("riscv64") {
        println!("cargo:rustc-link-arg=--defsym=HAMIX_USER_BASE=0x2000000000");
    }
}
