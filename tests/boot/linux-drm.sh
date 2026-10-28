echo "hxtest: hx-drm-compat on /dev/dri/card0"
if test -e /dev/dri/card0; then echo "PASS /dev/dri/card0 exists"; else echo "FAIL /dev/dri/card0 is missing"; fi
/linux/bin/drmcheck-musl
/linux/bin/drmcheck-musl-static
