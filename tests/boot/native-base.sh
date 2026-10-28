echo "hxtest: native base system"
for p in bash hxserver hxterm hxfiles pantry hxmon sudo su fetch gpuinfo cpuinfo startx lsblk ps uname mkfs.hext setconf ls cat grep; do
    if test -x /usr/bin/$p; then echo "PASS /usr/bin/$p"; else echo "FAIL /usr/bin/$p is missing"; fi
done
for p in /bin/sh /bin/bash; do
    if test -x $p; then echo "PASS $p"; else echo "FAIL $p is missing"; fi
done
if test -e /usr/bin/hsh; then echo "FAIL hsh is still installed"; else echo "PASS hsh is disabled"; fi
if test -f /etc/hamix/profile; then echo "PASS profile"; else echo "FAIL /etc/hamix/profile is missing"; fi
cat /etc/hamix/profile
uname -a
hxsound info
pantry --help
hxmon --help
lsblk
fetch
gpuinfo
cpuinfo | head -3
