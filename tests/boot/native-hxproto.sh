echo "hxtest: hxproto v2"
hxserver &
sleep 8
/usr/libexec/hamix/hxproto-check
