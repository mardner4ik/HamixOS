echo "hxtest: services moved out of the kernel"
/usr/lib/hamix/tests/bin/native-ipc
hxctl status
cmd add hello-test /usr/bin/hxsound info
cmd list
if test -f /var/lib/hamix/commands/hello-test; then echo "PASS command registered as a file"; else echo "FAIL command registry file missing"; fi
hello-test
cmd rm hello-test
if test -f /var/lib/hamix/commands/hello-test; then echo "FAIL command not removed"; else echo "PASS command removed"; fi
