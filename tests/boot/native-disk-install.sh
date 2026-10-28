echo "hxtest: install HamixOS to a blank disk with the bash installer"
lsblk
disk=$(lsblk | awk 'NR > 1 && $3 == "disk" { print $1; exit }')
if [ -z "$disk" ]; then echo "FAIL no disk attached"; exit 1; fi
echo "PASS target disk $disk"
conf=/tmp/install.conf
printf 'DISK=%s\nUSERNAME=anna\nUSER_PASSWORD=pa ss$word\nROOT_PASSWORD=r00t\nADMIN=yes\nHOSTNAME=testbox\nAUTOSTART=no\nLINUX=yes\n' "$disk" > $conf
bash /usr/share/hamix/install.sh $conf > /tmp/install.log 2>&1
status=$?
cat /tmp/install.log
if [ $status = 0 ] && grep -q '^@done' /tmp/install.log; then echo "PASS installer finished"; else echo "FAIL installer exited with $status"; exit 1; fi
mkdir -p /mnt/check
mount ${disk}1 /mnt/check || { echo "FAIL cannot mount the installed system"; exit 1; }
for f in /usr/bin/bash /bin/sh /usr/bin/sudo /usr/bin/busybox /lib/ld.hx /usr/lib/libhxc.so /boot/kernel.bin /boot/grub/grub.cfg; do
    if [ -s /mnt/check$f ]; then echo "PASS installed $f"; else echo "FAIL $f missing on the disk"; fi
done
grep '^anna:' /mnt/check/etc/passwd | grep -q ':/usr/bin/bash$' && echo "PASS anna's shell is bash" || echo "FAIL anna's shell: $(grep '^anna:' /mnt/check/etc/passwd)"
grep -q "^anna:x:1000:" /mnt/check/etc/group && ! grep -q "^user:" /mnt/check/etc/group && echo "PASS /etc/group follows useradd and userdel" || echo "FAIL /etc/group: $(cat /mnt/check/etc/group)"
[ "$(cat /mnt/check/etc/hostname)" = testbox ] && echo "PASS hostname" || echo "FAIL hostname"
[ -u /mnt/check/usr/bin/sudo ] && echo "PASS sudo stays setuid on the disk" || echo "FAIL sudo lost its setuid bit"
grep -q anna /mnt/check/etc/sudoers && echo "PASS anna is a sudoer" || echo "FAIL sudoers"
for g in audio video input inet power sysconf; do
    grep "^$g:" /mnt/check/etc/group | cut -d: -f4 | tr ',' '\n' | grep -q -x anna && echo "PASS anna is in $g" || echo "FAIL anna is not in $g: $(grep "^$g:" /mnt/check/etc/group)"
done
ls -ln /mnt/check/usr/bin/poweroff /mnt/check/usr/libexec/hamix/hxsysconf
[ "$(ls -ln /mnt/check/usr/bin/poweroff | cut -c1-10)" = "-rwsr-x---" ] && [ "$(ls -ln /mnt/check/usr/bin/poweroff | awk '{print $4}')" = 3004 ] && echo "PASS poweroff keeps root:power 4750 on the disk" || echo "FAIL poweroff mode or group changed"
[ "$(ls -ln /mnt/check/usr/libexec/hamix/hxsysconf | awk '{print $4}')" = 3005 ] && echo "PASS hxsysconf keeps the sysconf group" || echo "FAIL hxsysconf group changed"
[ "$(hxconf /mnt/check/etc/hamix/login.conf shell)" = /usr/bin/bash ] && echo "PASS login.conf shell" || echo "FAIL login.conf shell"
if [ -e /mnt/check/usr/bin/hsh ]; then echo "FAIL hsh was installed"; else echo "PASS no hsh on the disk"; fi
umount /mnt/check
echo "PASS done"
