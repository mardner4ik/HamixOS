#!/usr/bin/hsh
# HamixOS installer back end. The graphical installer (hxinstall) writes a
# KEY=value file and runs: hsh /usr/share/hamix/install.sh <file>
# Lines starting with @ are progress messages for the installer window.

if [ -z "$1" ]; then
    echo "usage: install.sh config-file"
    echo "keys: DISK HOSTNAME USERNAME USER_PASSWORD ROOT_PASSWORD ADMIN AUTOSTART"
    exit 2
fi

loadconf "$1"

if [ "$(whoami)" != "root" ]; then
    echo "@error the installer must run as root"
    exit 1
fi
if [ -z "$DISK" ]; then
    echo "@error no target disk selected"
    exit 1
fi
HOSTNAME=${HOSTNAME:-hamix}
AUTOSTART=${AUTOSTART:-no}
ADMIN=${ADMIN:-yes}
PART=/dev/${DISK}1
TARGET=/mnt/hamix-target

fail() {
    echo "@error $1"
    umount $TARGET 2>/dev/null
    exit 1
}

echo "@step 1 Preparing /dev/$DISK"
umount $TARGET 2>/dev/null
mkdir -p $TARGET
diskpart $DISK wipe || fail "cannot erase /dev/$DISK (is it in use?)"
diskpart $DISK mbr || fail "cannot write a partition table"
diskpart $DISK add rest hext || fail "cannot create the system partition"

echo "@step 2 Formatting $PART"
mkfs.hext -L hamix $PART || fail "cannot format $PART"
mount $PART $TARGET || fail "cannot mount $PART"

echo "@step 3 Copying the system"
for dir in usr etc bin sbin boot root var opt srv lib; do
    if [ -d /$dir ]; then
        echo "@detail /$dir"
        cp -a /$dir $TARGET/$dir || fail "copying /$dir failed"
    fi
done
for dir in dev proc sys run tmp mnt media home; do
    mkdir -p $TARGET/$dir
done
chmod 1777 $TARGET/tmp
rm -rf $TARGET/var/cache/nook
rm -f $TARGET/etc/commands
rm -f $TARGET/root/.hsh_history

echo "@step 4 Creating accounts"
userdel --root $TARGET user
if [ -n "$USERNAME" ]; then
    if [ "$ADMIN" = "yes" ]; then
        useradd --root $TARGET -G sudo -s /usr/bin/hsh $USERNAME || fail "cannot create $USERNAME"
    else
        useradd --root $TARGET -s /usr/bin/hsh $USERNAME || fail "cannot create $USERNAME"
    fi
    chpasswd --root $TARGET "$USERNAME" "$USER_PASSWORD" || fail "cannot set the password of $USERNAME"
fi
chpasswd --root $TARGET root "$ROOT_PASSWORD" || fail "cannot set the root password"
hostname --root $TARGET $HOSTNAME
setconf $TARGET/etc/hamix/login.conf autostart_desktop $AUTOSTART
setconf $TARGET/etc/os-release PRETTY_NAME "\"HamixOS 0.6\""
echo "Welcome to HamixOS. Type help for the list of commands, startx for the desktop." > $TARGET/etc/motd

echo "@step 5 Installing the boot loader"
bootinstall $DISK --root $TARGET || fail "cannot install the boot loader"

echo "@step 6 Writing everything to disk"
sync
umount $TARGET || fail "cannot unmount $PART"
echo "@done HamixOS is installed on /dev/$DISK"
