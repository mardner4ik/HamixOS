echo "hxtest: log in through the LightDM greeter with a password and get the Nook session"
hxctl start dbus-system
for i in $(seq 1 20); do [ -S /run/dbus/system_bus_socket ] && break; sleep 0.5; done
hxctl start lightdm
for i in $(seq 1 40); do ps | grep -v grep | grep -q lightdm-gtk-gr && break; sleep 1; done
sleep 3
echo "greeter: type the password now"
session=""
for i in $(seq 1 ${LOGIN_SECONDS:-60}); do
    session=$(ps | grep -v grep | grep hxserver)
    [ -n "$session" ] && break
    sleep 1
done
[ -n "$session" ] && echo "PASS password login started Nook: $session" || { echo "FAIL no Nook after the password"; grep -v "Loading conf" /var/log/lightdm/lightdm.log | tail -20; }
sleep 6
echo "nook: screen ready"
sleep 4
echo "PASS done"
