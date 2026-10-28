echo "hxtest: wlr-layer-shell and foreign-toplevel through hxwayland"
hxserver &
sleep 8
hxcalc &
sleep 4
/usr/libexec/hamix/wl-layer-check --toplevels --clipboard --protocols
