#include <dbus/dbus.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static int check(DBusConnection *bus, unsigned int uid, const char *action, int *authorized, int *challenge)
{
    DBusMessage *call = dbus_message_new_method_call("org.freedesktop.PolicyKit1", "/org/freedesktop/PolicyKit1/Authority",
                                                     "org.freedesktop.PolicyKit1.Authority", "CheckAuthorization");
    DBusMessageIter args, subject, dict, entry, variant, details;
    dbus_message_iter_init_append(call, &args);
    dbus_message_iter_open_container(&args, DBUS_TYPE_STRUCT, NULL, &subject);
    const char *kind = "unix-process";
    dbus_message_iter_append_basic(&subject, DBUS_TYPE_STRING, &kind);
    dbus_message_iter_open_container(&subject, DBUS_TYPE_ARRAY, "{sv}", &dict);
    const char *keys[] = { "pid", "uid" };
    dbus_uint32_t pid = (dbus_uint32_t)getpid();
    dbus_int32_t user = (dbus_int32_t)uid;
    for (int i = 0; i < 2; i++) {
        dbus_message_iter_open_container(&dict, DBUS_TYPE_DICT_ENTRY, NULL, &entry);
        dbus_message_iter_append_basic(&entry, DBUS_TYPE_STRING, &keys[i]);
        dbus_message_iter_open_container(&entry, DBUS_TYPE_VARIANT, i == 0 ? "u" : "i", &variant);
        if (i == 0)
            dbus_message_iter_append_basic(&variant, DBUS_TYPE_UINT32, &pid);
        else
            dbus_message_iter_append_basic(&variant, DBUS_TYPE_INT32, &user);
        dbus_message_iter_close_container(&entry, &variant);
        dbus_message_iter_close_container(&dict, &entry);
    }
    dbus_message_iter_close_container(&subject, &dict);
    dbus_message_iter_close_container(&args, &subject);
    dbus_message_iter_append_basic(&args, DBUS_TYPE_STRING, &action);
    dbus_message_iter_open_container(&args, DBUS_TYPE_ARRAY, "{ss}", &details);
    dbus_message_iter_close_container(&args, &details);
    dbus_uint32_t flags = 0;
    const char *cancel = "";
    dbus_message_iter_append_basic(&args, DBUS_TYPE_UINT32, &flags);
    dbus_message_iter_append_basic(&args, DBUS_TYPE_STRING, &cancel);
    DBusError error;
    dbus_error_init(&error);
    DBusMessage *reply = dbus_connection_send_with_reply_and_block(bus, call, 5000, &error);
    dbus_message_unref(call);
    if (!reply) {
        printf("polkit call failed: %s\n", error.message);
        dbus_error_free(&error);
        return -1;
    }
    DBusMessageIter it, result;
    dbus_message_iter_init(reply, &it);
    dbus_message_iter_recurse(&it, &result);
    dbus_bool_t a = 0, c = 0;
    dbus_message_iter_get_basic(&result, &a);
    dbus_message_iter_next(&result);
    dbus_message_iter_get_basic(&result, &c);
    *authorized = a;
    *challenge = c;
    dbus_message_unref(reply);
    return 0;
}

int main(void)
{
    DBusError error;
    dbus_error_init(&error);
    DBusConnection *bus = dbus_bus_get(DBUS_BUS_SYSTEM, &error);
    if (!bus) {
        printf("FAIL cannot reach the system bus: %s\n", error.message);
        return 1;
    }
    struct { unsigned int uid; const char *action; int authorized; int challenge; } cases[] = {
        { 0, "org.freedesktop.systemd1.manage-units", 1, 0 },
        { 1000, "org.freedesktop.login1.power-off", 1, 0 },
        { 1000, "org.freedesktop.udisks2.filesystem-mount", 1, 0 },
        { 1000, "org.freedesktop.systemd1.manage-units", 0, 1 },
        { 4242, "org.freedesktop.systemd1.manage-units", 0, 0 },
    };
    int failures = 0;
    for (unsigned i = 0; i < sizeof cases / sizeof cases[0]; i++) {
        int authorized = -1, challenge = -1;
        int r = check(bus, cases[i].uid, cases[i].action, &authorized, &challenge);
        int ok = r == 0 && authorized == cases[i].authorized && challenge == cases[i].challenge;
        printf("%s polkit uid %u %s -> authorized=%d challenge=%d\n", ok ? "PASS" : "FAIL", cases[i].uid, cases[i].action, authorized, challenge);
        failures += !ok;
    }
    return failures ? 1 : 0;
}
