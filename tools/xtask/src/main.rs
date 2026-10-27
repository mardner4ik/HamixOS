mod abi;
mod bench;
mod exfat;
mod golden;
mod hxlibc;
mod bootloader;
mod image;
mod isoimage;
mod lint;
mod ports;
mod qemu;
mod toolchain;
mod util;
mod ventoy;
mod workspace;

use std::path::PathBuf;
use std::time::Duration;

use image::{Arch, Desktop, Options, Profile};
use util::Result;

const HELP: &str = "cargo xtask <command> [options]

commands:
  build          kernel, userland, modules, root image and (x86_64) the ISO
  kernel         only the kernel
  run            boot the last built image in QEMU (serial on stdio)
  test           boot every tests/boot/*.sh script in QEMU and check its exit code
  lint-layers    check that nothing native depends on layers/linux (rule of the layer)
  lint-unsafe    count `unsafe` per subsystem; fails when a subsystem has more than tools/xtask/unsafe-baseline.txt
  abi            regenerate the kernel core table, hamix/syscall.h and hxlibc-sys numbers
                 from docs/reference/hamix-abi.md (--check only verifies)
  hxlibc-build   build hxlibc (mlibc + sysdeps/hamix) into out/hxlibc/ARCH/sysroot for hxlibc/hx-cc
  ports [NAME..] build the HXBUILD recipes in ports/*/* against hxlibc into out/hx-ports/ARCH/root
  kbench         kernel micro-benchmarks in QEMU (hxlibc and Linux/musl builds), out/bench/kbench.json
  membench       memory use after boot and with Nook idle, out/bench/membench.json
  prof [LOG]     symbolize a /proc/hxprof report (default: out/test/native-prof.sh.log)
  golden         compare Nook with the golden reference in tests/nook-golden (--update to record it)
  apps           list the userland programs, the ported recipes and where each lives

options:
  --arch A        x86_64 (default), aarch64, riscv64
  --profile P     full (default: native + Linux layer) or core (hamix-core, no Linux layer)
  --desktop D     all (default), nook or enlightenment: which desktop goes into the image
  --extra DIR     copy DIR on top of the root filesystem (also $HAMIX_EXTRA)
  --loader L      grub or hxboot: grub-mkrescue ISO or hxboot BIOS + GRUB UEFI ISO
  --memory M      QEMU memory for run/test (default 1G)
  --timeout S     seconds per boot test (default 120)
  --only NAME     run only boot tests whose file name contains NAME
  --net           also run the boot tests that need the internet (*-net-*.sh)
  --smp N         boot tests with N CPUs (default 2, 3 GiB RAM, like a laptop; stress-*.sh always use at least 4)
  -- ARGS         extra QEMU arguments for run
";

struct Args {
    command: String,
    arch: Arch,
    profile: Profile,
    desktop: Desktop,
    extra: Option<PathBuf>,
    memory: String,
    timeout: u64,
    only: Option<String>,
    check: bool,
    net: bool,
    no_build: bool,
    update: bool,
    smp: u32,
    names: Vec<String>,
    rest: Vec<String>,
}

fn parse() -> Result<Args> {
    let mut raw: Vec<String> = std::env::args().skip(1).collect();
    let mut rest = Vec::new();
    if let Some(pos) = raw.iter().position(|a| a == "--") {
        rest = raw.split_off(pos + 1);
        raw.pop();
    }
    let mut args = Args {
        command: String::new(),
        arch: Arch::X86_64,
        profile: Profile::Full,
        desktop: Desktop::All,
        extra: std::env::var_os("HAMIX_EXTRA").map(PathBuf::from),
        memory: String::from("1G"),
        timeout: 120,
        only: None,
        check: false,
        net: false,
        no_build: false,
        update: false,
        smp: 2,
        names: Vec::new(),
        rest,
    };
    let mut it = raw.into_iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{} needs a value", name));
        match arg.as_str() {
            "--arch" => args.arch = Arch::parse(&value("--arch")?)?,
            "--profile" => args.profile = Profile::parse(&value("--profile")?)?,
            "--desktop" => args.desktop = Desktop::parse(&value("--desktop")?)?,
            "--extra" => args.extra = Some(PathBuf::from(value("--extra")?)),
            "--memory" | "-m" => args.memory = value("--memory")?,
            "--loader" => {
                let loader = value("--loader")?;
                bootloader::Loader::parse(&loader)?;
                std::env::set_var("HAMIX_LOADER", loader);
            }
            "--timeout" => args.timeout = value("--timeout")?.parse().map_err(|_| String::from("--timeout needs seconds"))?,
            "--only" => args.only = Some(value("--only")?),
            "--check" => args.check = true,
            "--net" => args.net = true,
            "--no-build" => args.no_build = true,
            "--update" => args.update = true,
            "--smp" => args.smp = value("--smp")?.parse().map_err(|_| String::from("--smp needs a number"))?,
            "-h" | "--help" | "help" => args.command = String::from("help"),
            other if other.starts_with('-') => return Err(format!("unknown option {}", other)),
            other if args.command.is_empty() => args.command = String::from(other),
            other if args.command == "ports" || args.command == "prof" => args.names.push(String::from(other)),
            other => return Err(format!("unexpected argument {}", other)),
        }
    }
    Ok(args)
}

fn main() {
    let result = parse().and_then(|args| {
        workspace::sync()?;
        let opts = Options { arch: args.arch, profile: args.profile, desktop: args.desktop, extra: args.extra.clone() };
        match args.command.as_str() {
            "build" | "iso" => image::build(&opts),
            "kernel" => image::kernel(args.arch).map(|_| ()),
            "run" => qemu::run(args.arch, args.profile, &args.memory, &args.rest),
            "test" => qemu::test(args.profile, args.only.as_deref(), Duration::from_secs(args.timeout), args.net, args.smp),
            "lint-layers" => lint::lint_layers(),
            "lint-unsafe" => lint::lint_unsafe(args.update),
            "abi" => abi::generate(args.check),
            "hxlibc-build" => hxlibc::build(args.arch),
            "ports" => ports::build(args.arch, &args.names),
            "kbench" => bench::kbench(!args.no_build),
            "golden" => golden::golden(args.update),
            "membench" => bench::membench(!args.no_build),
            "prof" => bench::symbolize(args.names.first().map(|s| s.as_str()).unwrap_or("out/test/native-prof.sh.log")),
            "apps" => {
                for app in image::apps() {
                    println!("{:<12} {:<7} {}", app.name, if app.linux_layer { "linux" } else { "native" }, app.dir.strip_prefix(util::root()).unwrap_or(&app.dir).display());
                    if app.bins.len() > 1 {
                        println!("{:<12} {:<7} programs: {}", "", "", app.bins.join(" "));
                    }
                }
                for lib in image::clibs() {
                    println!("{:<12} {:<7} {} lib{}.so.{}", lib.name, "clib", lib.dir.strip_prefix(util::root()).unwrap_or(&lib.dir).display(), lib.name, lib.soversion);
                }
                for name in image::disabled_apps() {
                    println!("{:<12} {:<7} (disabled)", name, "native");
                }
                for recipe in ports::recipes()? {
                    println!("{:<12} {:<7} {} {}", recipe.name, "port", recipe.dir.strip_prefix(util::root()).unwrap_or(&recipe.dir).display(), recipe.version);
                }
                Ok(())
            }
            "" | "help" => {
                print!("{}", HELP);
                Ok(())
            }
            other => Err(format!("unknown command '{}'\n\n{}", other, HELP)),
        }
    });
    if let Err(e) = result {
        eprintln!("[ERROR] {}", e);
        std::process::exit(1);
    }
}
