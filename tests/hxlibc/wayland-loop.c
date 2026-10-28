#include <wayland-client.h>
#include <wayland-server.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

static int seats;

static void bind_seat(struct wl_client *client, void *data, uint32_t version, uint32_t id)
{
    struct wl_resource *resource = wl_resource_create(client, &wl_seat_interface, version, id);
    if (resource)
        wl_seat_send_capabilities(resource, WL_SEAT_CAPABILITY_KEYBOARD);
}

static void on_global(void *data, struct wl_registry *registry, uint32_t name, const char *interface, uint32_t version)
{
    if (strcmp(interface, "wl_seat") == 0)
        seats++;
}

static void on_global_remove(void *data, struct wl_registry *registry, uint32_t name)
{
}

static const struct wl_registry_listener registry_listener = { on_global, on_global_remove };

static int run_client(const char *socket)
{
    struct wl_display *display = wl_display_connect(socket);
    if (!display)
        return 2;
    struct wl_registry *registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &registry_listener, NULL);
    if (wl_display_roundtrip(display) < 0)
        return 3;
    wl_registry_destroy(registry);
    wl_display_disconnect(display);
    return seats == 1 ? 0 : 4;
}

int main(void)
{
    mkdir("/tmp/wayland-test", 0700);
    setenv("XDG_RUNTIME_DIR", "/tmp/wayland-test", 1);
    struct wl_display *display = wl_display_create();
    const char *socket = display ? wl_display_add_socket_auto(display) : NULL;
    if (!socket) {
        printf("FAIL wayland server socket: %s\n", strerror(errno));
        return 1;
    }
    printf("PASS wayland server listens on %s\n", socket);
    wl_global_create(display, &wl_seat_interface, 1, NULL, bind_seat);
    pid_t child = fork();
    if (child == 0)
        _exit(run_client(socket));
    struct wl_event_loop *loop = wl_display_get_event_loop(display);
    int status = -1;
    for (int i = 0; i < 400; i++) {
        wl_event_loop_dispatch(loop, 10);
        wl_display_flush_clients(display);
        if (waitpid(child, &status, WNOHANG) == child)
            break;
    }
    int ok = WIFEXITED(status) && WEXITSTATUS(status) == 0;
    printf("%s wayland client saw the wl_seat global over the socket (status %d)\n", ok ? "PASS" : "FAIL", WIFEXITED(status) ? WEXITSTATUS(status) : -1);
    wl_display_destroy(display);
    return ok ? 0 : 1;
}
