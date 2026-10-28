echo "hxtest: hamix-core boots with linux=0"
hxctl linux status
if test -e /dev/dri; then echo "FAIL /dev/dri exists without the Linux layer"; else echo "PASS no /dev/dri without the Linux layer"; fi
hxctl abi
