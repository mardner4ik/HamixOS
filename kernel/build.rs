fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let target = std::env::var("TARGET").unwrap_or_default();

    if target.starts_with("aarch64") {
        println!("cargo:rerun-if-changed=linker_aarch64.ld");
        println!("cargo:rustc-link-arg=-T{}/linker_aarch64.ld", manifest_dir);
        return;
    }

    println!("cargo:rerun-if-changed=src/arch/x86_64/boot.S");
    println!("cargo:rerun-if-changed=linker.ld");
    println!("cargo:rustc-link-arg=-T{}/linker.ld", manifest_dir);
    cc::Build::new()
        .file("src/arch/x86_64/boot.S")
        .flag("-m64")
        .flag("-ffreestanding")
        .flag("-fno-pic")
        .flag("-fno-pie")
        .compile("boot");
}
