# Dynamic command registry

Any program can, at any moment, publish a command: a name that links to a
program with preset arguments. The registry lives in the kernel
(`kernel/src/task/registry.rs`) and is stored in `/etc/commands`, so on a
persistent hext root it survives reboots.

## From the shell

```
cmd                          list registered commands
cmd add notes hed /home/user/notes.txt
cmd rm notes
notes                        runs /usr/bin/hed /home/user/notes.txt
notes --view                 extra arguments are appended
```

Shell builtins always win; names of builtins cannot be registered. Unknown
words are looked up in `/usr/bin`, `/bin`, `/sbin` first, then in the
registry.

## From a program (`hamix_std::sys`)

```rust
sys::cmd_register("hello-window", "/usr/bin/hxhello", &["Hi!"]);
sys::cmd_unregister("hello-window");
for c in sys::cmd_list() { /* c.name, c.path, c.args */ }
sys::cmd_run("hello-window", &["extra"], sys::SPAWN_DETACH);
```

| Syscall | Number | Arguments |
|---------|--------|-----------|
| `cmd_register` | 9040 | name ptr, name len, NUL-separated block `program\0arg\0...`, block len |
| `cmd_unregister` | 9041 | name ptr, name len |
| `cmd_list` | 9042 | buffer, length → `name\tpath\targs...\n` lines |
| `cmd_run` | 9043 | name ptr, name len, extra args block, `len | flags << 32` → pid |

Names are 1–32 characters from `[A-Za-z0-9._-]`. A command belongs to the
uid that created it; only that user or root can replace or remove it.

Programs shipped with HamixOS that use it:

* `hed` registers `hedlast`, reopening the last edited file;
* `hxhello` registers `hello-window`;
* `hxfiles` has a *Pin cmd* button that registers `files-<dir>`;
* the Nook menu lists registered commands and shows a notification when a new
  one appears; `hxcmd` runs and removes them.
