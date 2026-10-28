echo "hxtest: the Linux environment in /linux"
check() { if [ "$2" = "$3" ]; then echo "PASS $1"; else echo "FAIL $1: got '$2', want '$3'"; fi; }
cd /tmp
check "shared /tmp keeps the same path" "$(linux pwd)" "/tmp"
cd /usr
check "a native directory is seen under /hamix" "$(linux pwd)" "/hamix/usr"
cd /
check "Linux / is /linux" "$(linux /bin/busybox sh -c 'test -x /bin/busybox && test -x /lib/ld-musl-x86_64.so.1 && echo yes')" "yes"
check "native programs stay out of the Linux /usr/bin" "$(linux /bin/busybox sh -c 'test -e /usr/bin/hxterm && echo leaked || echo clean')" "clean"
check "Linux /usr/bin/bash is the native bash" "$(linux /usr/bin/bash -c 'echo $BASH_VERSINFO')" "5"
check "native programs are reachable through /hamix" "$(linux /bin/busybox sh -c 'test -x /hamix/usr/bin/bash && echo yes')" "yes"
echo shared-data > /tmp/shared.txt
check "files in /tmp are shared" "$(linux cat /tmp/shared.txt)" "shared-data"
linux /bin/busybox sh -c 'echo inside > /etc/linux-only.conf'
if [ -f /linux/etc/linux-only.conf ] && [ ! -e /etc/linux-only.conf ]; then echo "PASS Linux /etc is /linux/etc"; else echo "FAIL Linux /etc leaked into the native /etc"; fi
check "user accounts are shared" "$(linux /bin/busybox sh -c 'grep -c "^root:" /etc/passwd')" "1"
check "readlink of /proc/self/exe is in the Linux view" "$(linux readlink /proc/self/exe)" "/bin/busybox"
mkdir -p /linux/usr/local/bin
ln -s /bin/busybox /linux/usr/local/bin/ls
check "absolute symlinks inside /linux resolve inside /linux" "$(linux /usr/local/bin/ls -d /lib)" "/lib"
check "Linux scripts run with the Linux shell" "$(linux /bin/busybox sh -c 'printf "#!/bin/sh\necho \$0\n" > /tmp/l.sh; chmod 755 /tmp/l.sh; /tmp/l.sh')" "/tmp/l.sh"
if [ -e /usr/bin/busybox ] && [ ! -e /bin/busybox ]; then echo "PASS native /bin has no Linux busybox"; else echo "FAIL native /bin still has Linux files"; fi
