echo "hxtest: forked and duplicated descriptors share one file offset"
f() { echo in; cat /etc/hostname; }
{ echo a; f 2>&1; echo b; } > /tmp/offset.txt
got=$(tr '\n' ' ' < /tmp/offset.txt)
want="a in $(cat /etc/hostname) b "
[ "$got" = "$want" ] || echo "FAIL got '$got' want '$want'"
exec 3>/tmp/offset2.txt
exec 4>&3
echo one >&3
echo two >&4
/bin/sh -c 'echo three >&3'
exec 3>&- 4>&-
got=$(tr '\n' ' ' < /tmp/offset2.txt)
[ "$got" = "one two three " ] || echo "FAIL dup got '$got'"
echo "PASS done"
