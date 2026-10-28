#!/bin/sh
set -e

if [ "$(id -u)" != 0 ]; then
	echo "run as root: doas sh $0 [user]" >&2
	exit 1
fi

user=${1:-${DOAS_USER:-$(logname)}}

cat > /etc/login.conf.d/hamixbuild <<'CLASS'
hamixbuild:\
	:datasize-cur=infinity:\
	:datasize-max=infinity:\
	:stacksize-cur=16M:\
	:maxproc-cur=1024:\
	:maxproc-max=2048:\
	:openfiles-cur=8192:\
	:openfiles-max=16384:\
	:tc=staff:
CLASS
if [ -f /etc/login.conf.db ]; then
	cap_mkdb /etc/login.conf
fi
usermod -L hamixbuild "$user"

touch /etc/sysctl.conf
for setting in hw.smt=1 kern.pool_debug=0 kern.maxfiles=65536; do
	sysctl "$setting"
	key=${setting%%=*}
	grep -v "^$key=" /etc/sysctl.conf > /etc/sysctl.conf.new || true
	echo "$setting" >> /etc/sysctl.conf.new
	mv /etc/sysctl.conf.new /etc/sysctl.conf
done

if ! awk '$2 == "/home" { print $4 }' /etc/fstab | grep -q noatime; then
	cp /etc/fstab /etc/fstab.before-hamix
	awk '$2 == "/home" && $4 !~ /noatime/ { $4 = $4 ",noatime" } { print }' /etc/fstab.before-hamix > /etc/fstab
	mount -u -o noatime /home
fi

pkg_add -I coreutils gtar meson ninja xorriso mtools nasm

echo "done: log out and back in as $user so the hamixbuild login class applies"
