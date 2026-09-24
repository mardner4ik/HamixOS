# hext: the HamixOS filesystem

hext is HamixOS's own filesystem. On disk it is laid out exactly like
**ext2 revision 1** (superblock at byte 1024, block groups, bitmaps, 128-byte
inodes, direct/indirect block maps, linear directories with file types), so
`e2fsck`, `debugfs` and `dumpe2fs` can inspect a hext image. On top of that
it adds protection against sudden power loss.

The implementation is a standalone `no_std` crate, `libs/hext`, used both by
the kernel and by the host tool `tools/mkhext`.

## Crash safety

1. **Copy-on-write file data.** Rewriting a file never overwrites blocks the
   committed filesystem still references: new data goes to freshly allocated
   blocks, and blocks freed in the current transaction cannot be reused until
   it commits.
2. **Journaled metadata.** Every metadata block changed by an operation
   (inode tables, bitmaps, group descriptors, superblock, directory blocks)
   is staged in memory and written as one transaction to a journal kept in
   reserved inode 5:
   descriptor blocks (targets + CRC32) → the new block images → a commit
   block (CRC32 over all data). Only after the commit block is flushed are the
   blocks written to their home locations, and only then is the journal
   header advanced.
3. **Replay on mount.** A transaction with a valid commit record that was not
   checkpointed yet is replayed; anything without a valid commit record is
   ignored, so the filesystem shows either the old or the new state of an
   operation, never a mix.

`libs/hext/tests/fs.rs` checks this by simulating a power cut after every
single block write of a transaction, remounting, verifying that the result is
atomic and running `e2fsck -fn` on it.

hext-specific fields live in the reserved area of the superblock at offset
`0x300`: magic `HEXT`, version, journal inode, a CRC32 of the superblock and
a mount counter.

## How the kernel uses it

* The root volume is chosen by the `root=` kernel option (see
  [INSTALL.md](INSTALL.md)): the live image in RAM, or a hext partition on a
  SATA/IDE disk found by UUID, label or device name.
* Further hext volumes can be mounted anywhere with `mount /dev/sdXN dir`;
  each mounted volume has its own backend and journal, `umount` flushes it.
* An installed root (and any other mounted volume) is loaded lazily: the
  directory tree is read at mount time, but only files up to 16 KiB (link
  files among them) are loaded; everything else is read from disk when it is
  first opened and dropped again by the file cache (1/8 of free memory,
  16-96 MiB). Files over 4 MiB are read in pieces without entering the cache.
  Before this change the whole disk was copied into RAM at boot.
* The VFS lock (`fs::VFS`) yields to other tasks when it is contended, so a
  task may do disk I/O while holding it.
* Deleted files are freed: `hextd` looks for file nodes that are no longer
  reachable from `/` and that no descriptor, mapping or in-flight
  `SCM_RIGHTS` message references, twice in a row, and drops their data.
* The VFS keeps file contents in memory and tracks dirty nodes per volume.
  The kernel thread `hextd` takes a snapshot of the changes, writes them to
  disk and commits a transaction; it yields between writes so the desktop
  keeps running. `sync`, `sys::sync()`, logout, `reboot` and `poweroff` flush
  immediately.
* `/dev`, `/proc`, `/sys`, `/run` and `/tmp` are never written to disk.

## Tools

```
mkhext [--size 256M] [--label NAME] <source-dir> <image>
mkfs.hext [-L label] /dev/sdXN      # inside HamixOS
```

`build.sh` creates `isoroot/boot/hext.img.gz` from `rootfs/`; home
directories get the owners listed in `rootfs/etc/passwd`.
