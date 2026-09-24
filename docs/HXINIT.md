# hxinit

`hxinit` (`kernel/src/hxinit.rs`) is the system initialisation program. It is
built into the kernel and runs as the first kernel thread (pid 1, pinned to
CPU 0). Everything that used to be scattered through `kernel_main` is now a
*unit* with a name, a title, a result and the time it took.

On boot the console shows one line per unit:

```
[  OK  ] Processors - 4 cores online, Intel(R) Core(TM) i5 ...
[ SKIP ] USB host controllers - none
[ WARN ] Network adapters - no driver for 10ec:8168
[FAILED] Root filesystem - UUID=... not found
```

followed by a summary: `hxinit 23 units, 0 warnings, 0 failed, 1188 ms`.

## Units, in order

| Unit | What it does |
|------|--------------|
| `memory`, `cpu`, `interrupts`, `framebuffer` | recorded by the early kernel before hxinit starts |
| `rtc` | real-time clock |
| `acpi` | RSDP, MADT (processors, I/O APIC), FADT (power off / reset) |
| `pci` | bus scan |
| `graphics` | Intel graphics driver and display modes |
| `vfs`, `storage`, `rootfs`, `devices` | filesystem, AHCI/IDE disks, root (`root=`), `/dev` |
| `accounts` | `/etc/passwd`, `/etc/shadow` |
| `keyboard`, `mouse`, `usb` | PS/2 and USB input |
| `registry` | command registry |
| `hextd`, `usbd` | filesystem sync and USB hotplug threads |
| `smp` | starts the application processors (INIT/SIPI) |
| `display` | applies `/etc/hamix/display.conf` |
| `netdev`, `netstack` | network adapters and the smoltcp TCP/IP stack |
| `audio` | sound controller (Intel HDA or AC'97), codec and outputs; starts the `audiod` mixer thread |
| `login` | login prompt on tty1 |

After `login` hxinit stays alive as a supervisor: once per second it checks
pending display-mode trials (the 15 s auto-revert).

## Status

* `/proc/hxinit` -- one unit per line: `name  status  ms  title  detail`
  (tab-separated, status is `ok`, `warn`, `fail` or `skip`).
* Settings → System status shows the same list.

## Adding a unit

```rust
hxinit::run("name", "Title shown on the console", || {
    match do_something() {
        Ok(detail) => hxinit::ok(detail),
        Err(e) => hxinit::fail(e),
    }
});
```

`hxinit::warn` marks something that works in a reduced way, `hxinit::skip`
something that is not present. `run_quiet` runs the closure with interrupts
disabled.
