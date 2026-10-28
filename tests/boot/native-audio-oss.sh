echo "hxtest: OSS /dev/dsp and /dev/mixer"
for node in /dev/dsp /dev/mixer; do
    if test -c $node; then echo "PASS $node is a character device"; else echo "FAIL $node is missing"; fi
done
/usr/lib/hamix/tests/bin/oss-check || echo "FAIL oss-check exited with $?"
hxsound=$(command -v hxsound || echo /usr/libexec/hamix/hxsound)
out=$($hxsound volume 70)
echo "$out"
case "$out" in
    *"volume 70%"*) echo "PASS hxsound sets the volume through /dev/mixer" ;;
    *) echo "FAIL hxsound volume 70 printed: $out" ;;
esac
$hxsound info || echo "FAIL hxsound info exited with $?"
$hxsound tone 660 0.3 || echo "FAIL hxsound tone exited with $?"
