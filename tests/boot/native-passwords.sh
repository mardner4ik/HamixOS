echo "hxtest: passwords as crypt(3) SHA-512 in /etc/shadow, checked by hx-authd, no accounts in the kernel"
/usr/lib/hamix/tests/bin/crypt-check || echo "FAIL crypt-check exited with $?"
grep -q '^root:\$6\$' /etc/shadow && echo "PASS root hash is SHA-512 crypt" || echo "FAIL root hash: $(cut -c1-20 /etc/shadow | head -1)"
echo "user:Sekret-42" | chpasswd && echo "PASS chpasswd" || echo "FAIL chpasswd"
grep -q '^user:\$6\$[^:]*:[0-9]*:0:99999:7:::$' /etc/shadow && echo "PASS shadow line has the Linux layout" || echo "FAIL shadow line: $(grep '^user:' /etc/shadow | cut -d: -f3-)"
out=$(printf 'wrong\nwrong\nwrong\n' | /usr/libexec/hamix/hxsvc --as user -- /usr/bin/sudo -S /usr/bin/id -u 2>&1)
[ "$(echo "$out" | tail -1)" = "0" ] && echo "FAIL sudo accepted a wrong password" || echo "PASS sudo rejects a wrong password"
out=$(printf 'Sekret-42\n' | /usr/libexec/hamix/hxsvc --as user -- /usr/bin/sudo -S /usr/bin/id -u 2>&1)
[ "$(echo "$out" | tail -1)" = "0" ] && echo "PASS sudo accepts the new password" || echo "FAIL sudo: $out"
echo "PASS done"
