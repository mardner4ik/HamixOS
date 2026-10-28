echo "hxtest: Unix groups, devfs.conf, setuid power tools, hxperm-exec and hxsysconf"
check() { if eval "$2"; then echo "PASS $1"; else echo "FAIL $1"; fi; }
as_user() { /usr/libexec/hamix/hxsvc --as "$1" -- /usr/bin/bash -c "$2"; }
gid() { awk -F: -v g="$1" '$1 == g { print $3 }' /etc/group; }

id user
check "user is in the default permission groups" '[ "$(id -Gn user | tr " " "\n" | grep -c -x -e audio -e video -e input -e inet -e power -e sysconf)" = 6 ]'
ls -ln /usr/bin/poweroff /usr/bin/reboot /usr/libexec/hamix/hxperm-exec /usr/libexec/hamix/hxsysconf
check "poweroff is setuid root:power 4750" '[ "$(ls -ln /usr/bin/poweroff | cut -c1-10)" = "-rwsr-x---" ] && [ "$(ls -ln /usr/bin/poweroff | awk "{print \$4}")" = "$(gid power)" ]'
check "hxsysconf is setuid root:sysconf 4750" '[ "$(ls -ln /usr/libexec/hamix/hxsysconf | cut -c1-10)" = "-rwsr-x---" ] && [ "$(ls -ln /usr/libexec/hamix/hxsysconf | awk "{print \$4}")" = "$(gid sysconf)" ]'
check "hxperm-exec is setuid root 4755" '[ "$(ls -ln /usr/libexec/hamix/hxperm-exec | cut -c1-10)" = "-rwsr-xr-x" ]'
ls -ln /dev/dsp /dev/fb0 2>&1
check "devfs.conf gave /dev/dsp to the audio group" '[ ! -e /dev/dsp ] || [ "$(ls -ln /dev/dsp | awk "{print \$4}")" = "$(gid audio)" ]'

session=$(as_user user 'id -G')
echo "user session groups: $session"
check "login sessions get supplementary groups" 'echo " $session " | grep -q " $(gid inet) "'
calc=$(as_user user '/usr/libexec/hamix/hxperm-exec org.hamix.Calculator /usr/bin/id -G')
echo "calculator groups: $calc"
check "an app without permissions loses the permission groups" '! echo " $calc " | grep -q -e " $(gid inet) " -e " $(gid audio) " -e " $(gid power) "'
check "hxperm-exec drops back to the user" '[ "$(as_user user "/usr/libexec/hamix/hxperm-exec org.hamix.Calculator /usr/bin/id -u")" = "$(id -u user)" ]'
term=$(as_user user '/usr/libexec/hamix/hxperm-exec org.hamix.Terminal /usr/bin/id -G')
check "the terminal keeps every group it asks for" 'echo " $term " | grep -q " $(gid power) " && echo " $term " | grep -q " $(gid inet) "'
denied=$(as_user user '/usr/libexec/hamix/hxperm-exec org.hamix.Calculator /usr/bin/ping -c 1 10.0.2.2 2>&1; echo status=$?')
echo "$denied"
check "an app without Internet cannot open sockets" '! echo "$denied" | grep -q "status=0$"'

ls /dev/input/ 2>&1 | head -3
ev=$(ls /dev/input/event* 2>/dev/null | head -1)
if [ -n "$ev" ]; then
    check "an app without Direct input access cannot open $ev" '! as_user user "/usr/libexec/hamix/hxperm-exec org.hamix.Calculator /usr/bin/dd if=$ev of=/dev/null bs=1 count=0" 2>/dev/null'
    check "the terminal may open $ev" 'as_user user "/usr/libexec/hamix/hxperm-exec org.hamix.Terminal /usr/bin/dd if=$ev of=/dev/null bs=1 count=0" 2>/dev/null'
fi
check "an app without Direct graphics access cannot open /dev/fb0" '! as_user user "/usr/libexec/hamix/hxperm-exec org.hamix.Calculator /usr/bin/dd if=/dev/fb0 of=/dev/null bs=4 count=1" 2>/dev/null'
check "the terminal may open /dev/fb0" 'as_user user "/usr/libexec/hamix/hxperm-exec org.hamix.Terminal /usr/bin/dd if=/dev/fb0 of=/dev/null bs=4 count=1" 2>/dev/null'
out=$(as_user user '/usr/libexec/hamix/hxsysconf permit org.hamix.Videos audio no; echo status=$?')
echo "$out"
check "a sysconf member revokes a permission without a password" 'echo "$out" | grep -q status=0 && [ -f /etc/hamix/permissions/org.hamix.Videos ]'
as_user user '/usr/libexec/hamix/hxsysconf show org.hamix.Videos'
check "the revoked permission is enforced" '! as_user user "/usr/libexec/hamix/hxperm-exec org.hamix.Videos /usr/bin/id -G" | tr " " "\n" | grep -q -x "$(gid audio)"'
check "required permissions cannot be revoked" '! as_user user "/usr/libexec/hamix/hxsysconf permit org.hamix.Nook video no" 2>/dev/null'
check "apps cannot get permissions they do not ask for" '! as_user user "/usr/libexec/hamix/hxsysconf permit org.hamix.Calculator inet yes" 2>/dev/null'
as_user user '/usr/libexec/hamix/hxsysconf reset org.hamix.Videos'
check "reset restores the declared permissions" '[ ! -e /etc/hamix/permissions/org.hamix.Videos ]'
check "setconf through hxsysconf is limited" '! as_user user "/usr/libexec/hamix/hxsysconf setconf /etc/passwd x y" 2>/dev/null'

useradd guest -p guestpw > /dev/null
id guest
check "a standard account is not in power or sysconf" '! id -Gn guest | tr " " "\n" | grep -q -x -e power -e sysconf'
check "access(2) reports hxsysconf as not executable for guests" 'as_user guest "[ ! -x /usr/libexec/hamix/hxsysconf ]"'
check "access(2) reports hxsysconf as executable for admins" 'as_user user "[ -x /usr/libexec/hamix/hxsysconf ]"'
check "guests cannot run hxsysconf" '! as_user guest "/usr/libexec/hamix/hxsysconf permit org.hamix.Videos audio no" 2>/dev/null'
check "guests cannot run poweroff" '! as_user guest "/usr/bin/poweroff" 2>/dev/null'
usermod -aG power guest
check "usermod -aG adds a group" 'id -Gn guest | tr " " "\n" | grep -q -x power'
usermod -G audio guest
check "usermod -G replaces the groups" 'id -Gn guest | tr " " "\n" | grep -q -x audio && ! id -Gn guest | tr " " "\n" | grep -q -x power'
userdel guest
check "userdel removes group memberships" '! grep -q guest /etc/group'
echo "PASS done"
