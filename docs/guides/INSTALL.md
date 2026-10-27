# Installing HamixOS

## From the desktop

Boot the ISO ("HamixOS 0.4 Live"), log in as `user` / `user`, run `startx` and
double-click **Install HamixOS**. The installer asks for:

1. the disk (SATA/AHCI or IDE; the whole disk is erased),
2. the user name and password, whether the user is an administrator (sudo),
   and the root password,
3. the computer name and whether Nook starts right after login,
4. confirmation and the password of the live user (the installer runs its
   back end as root through `sys::auth`).

## What happens

`hxinstall` writes a `KEY=value` file to `/tmp` (mode 600) and runs
`hsh /usr/share/hamix/install.sh <file>`. The script prints `@step`,
`@detail`, `@error` and `@done` lines that the window turns into progress.
Everything it does is available in the shell:

```
diskpart sda wipe                 # clear MBR, boot gap, GPT copies
diskpart sda mbr                  # new MBR
diskpart sda add rest hext        # /dev/sda1 from sector 2048, bootable
mkfs.hext -L hamix /dev/sda1
mount /dev/sda1 /mnt/hamix-target
cp -a /usr /mnt/hamix-target/usr  # …and etc, bin, boot, root, var, …
userdel --root /mnt/hamix-target user
useradd --root /mnt/hamix-target -G sudo NAME
chpasswd --root /mnt/hamix-target NAME PASSWORD
chpasswd --root /mnt/hamix-target root PASSWORD
hostname --root /mnt/hamix-target NAME
setconf /mnt/hamix-target/etc/hamix/login.conf autostart_desktop yes
bootinstall sda --root /mnt/hamix-target
umount /mnt/hamix-target
```

## Boot loader

`build.sh` creates `/usr/lib/hamix/boot/core.img` with `grub-mkimage`
(modules for BIOS disks, MBR partitions, ext2 — hext is ext2-compatible —
multiboot2 and video) and a prefix of `(,msdos1)/boot/grub`. `bootinstall`
writes GRUB's `boot.img` into the first 440 bytes of the MBR (the partition
table is kept), `core.img` into the gap after the MBR, copies
`/boot/kernel.bin` and writes `/boot/grub/grub.cfg` with
`root=UUID=<volume uuid>`.

## The `root=` kernel option

| value | meaning |
|---|---|
| `live` | use the image loaded by GRUB (`hext.img.gz`) in RAM |
| `auto` | first hext volume on any disk, otherwise the image |
| `UUID=…`, `LABEL=…`, `/dev/sda1` | that volume |

The ISO menu has a "Live" entry (`root=live`) and one that boots an
installed system from the disk (`root=auto`).
