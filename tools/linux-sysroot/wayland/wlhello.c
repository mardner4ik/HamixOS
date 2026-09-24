#define _GNU_SOURCE
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
#include <wayland-client.h>
#include "xdg-shell-client-protocol.h"

struct app {
    struct wl_display *display;
    struct wl_compositor *compositor;
    struct wl_shm *shm;
    struct wl_seat *seat;
    struct xdg_wm_base *wm_base;
    struct wl_surface *surface;
    struct xdg_surface *xdg_surface;
    struct xdg_toplevel *toplevel;
    int width, height;
    int pending_width, pending_height;
    int running;
    int frames;
    int clicks;
    int pointer_x, pointer_y;
    uint32_t hue;
};

static void buffer_release(void *data, struct wl_buffer *buffer) {
    (void)data;
    wl_buffer_destroy(buffer);
}

static const struct wl_buffer_listener buffer_listener = { buffer_release };

static struct wl_buffer *draw(struct app *app) {
    int stride = app->width * 4;
    int size = stride * app->height;
    int fd = memfd_create("wlhello", MFD_CLOEXEC);
    if (fd < 0 || ftruncate(fd, size) < 0) {
        perror("memfd");
        exit(1);
    }
    uint32_t *pixels = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (pixels == MAP_FAILED) {
        perror("mmap");
        exit(1);
    }
    for (int y = 0; y < app->height; y++) {
        for (int x = 0; x < app->width; x++) {
            uint32_t r = (x * 255 / app->width + app->hue) & 0xff;
            uint32_t g = (y * 255 / app->height) & 0xff;
            uint32_t b = (app->clicks * 60 + 80) & 0xff;
            int dx = x - app->pointer_x, dy = y - app->pointer_y;
            if (dx * dx + dy * dy < 400) {
                r = g = b = 255;
            }
            pixels[y * app->width + x] = 0xff000000 | (r << 16) | (g << 8) | b;
        }
    }
    munmap(pixels, size);
    struct wl_shm_pool *pool = wl_shm_create_pool(app->shm, fd, size);
    struct wl_buffer *buffer = wl_shm_pool_create_buffer(pool, 0, app->width, app->height, stride, WL_SHM_FORMAT_XRGB8888);
    wl_shm_pool_destroy(pool);
    close(fd);
    wl_buffer_add_listener(buffer, &buffer_listener, NULL);
    return buffer;
}

static const struct wl_callback_listener frame_listener;

static void redraw(struct app *app) {
    struct wl_callback *cb = wl_surface_frame(app->surface);
    wl_callback_add_listener(cb, &frame_listener, app);
    wl_surface_attach(app->surface, draw(app), 0, 0);
    wl_surface_damage_buffer(app->surface, 0, 0, app->width, app->height);
    wl_surface_commit(app->surface);
}

static void frame_done(void *data, struct wl_callback *cb, uint32_t time) {
    struct app *app = data;
    (void)time;
    wl_callback_destroy(cb);
    app->hue = (app->hue + 3) & 0xff;
    app->frames++;
    if (app->frames == 60) {
        printf("wlhello: 60 frames presented\n");
        fflush(stdout);
    }
    redraw(app);
}

static const struct wl_callback_listener frame_listener = { frame_done };

static void wm_ping(void *data, struct xdg_wm_base *base, uint32_t serial) {
    (void)data;
    xdg_wm_base_pong(base, serial);
}

static const struct xdg_wm_base_listener wm_listener = { wm_ping };

static void xdg_surface_configure(void *data, struct xdg_surface *surface, uint32_t serial) {
    struct app *app = data;
    xdg_surface_ack_configure(surface, serial);
    int first = app->width == 0;
    if (app->pending_width > 0 && app->pending_height > 0) {
        app->width = app->pending_width;
        app->height = app->pending_height;
    } else if (first) {
        app->width = 480;
        app->height = 320;
    }
    if (first) {
        printf("wlhello: configured %dx%d\n", app->width, app->height);
        fflush(stdout);
        redraw(app);
    }
}

static const struct xdg_surface_listener xdg_surface_listener = { xdg_surface_configure };

static void toplevel_configure(void *data, struct xdg_toplevel *toplevel, int32_t width, int32_t height, struct wl_array *states) {
    struct app *app = data;
    (void)toplevel;
    (void)states;
    app->pending_width = width;
    app->pending_height = height;
}

static void toplevel_close(void *data, struct xdg_toplevel *toplevel) {
    struct app *app = data;
    (void)toplevel;
    app->running = 0;
}

static void toplevel_bounds(void *data, struct xdg_toplevel *toplevel, int32_t w, int32_t h) {
    (void)data; (void)toplevel; (void)w; (void)h;
}

static void toplevel_caps(void *data, struct xdg_toplevel *toplevel, struct wl_array *caps) {
    (void)data; (void)toplevel; (void)caps;
}

static const struct xdg_toplevel_listener toplevel_listener = { toplevel_configure, toplevel_close, toplevel_bounds, toplevel_caps };

static void pointer_enter(void *data, struct wl_pointer *p, uint32_t serial, struct wl_surface *s, wl_fixed_t x, wl_fixed_t y) {
    struct app *app = data;
    (void)p; (void)serial; (void)s;
    app->pointer_x = wl_fixed_to_int(x);
    app->pointer_y = wl_fixed_to_int(y);
}
static void pointer_leave(void *data, struct wl_pointer *p, uint32_t serial, struct wl_surface *s) { (void)data; (void)p; (void)serial; (void)s; }
static void pointer_motion(void *data, struct wl_pointer *p, uint32_t time, wl_fixed_t x, wl_fixed_t y) {
    struct app *app = data;
    (void)p; (void)time;
    app->pointer_x = wl_fixed_to_int(x);
    app->pointer_y = wl_fixed_to_int(y);
}
static void pointer_button(void *data, struct wl_pointer *p, uint32_t serial, uint32_t time, uint32_t button, uint32_t state) {
    struct app *app = data;
    (void)p; (void)serial; (void)time;
    if (state == WL_POINTER_BUTTON_STATE_PRESSED) {
        app->clicks++;
        printf("wlhello: button %#x at %d,%d\n", button, app->pointer_x, app->pointer_y);
        fflush(stdout);
    }
}
static void pointer_axis(void *data, struct wl_pointer *p, uint32_t time, uint32_t axis, wl_fixed_t value) {
    (void)data; (void)p; (void)time;
    printf("wlhello: scroll axis %u value %d\n", axis, wl_fixed_to_int(value));
    fflush(stdout);
}
static void pointer_frame(void *data, struct wl_pointer *p) { (void)data; (void)p; }
static void pointer_axis_source(void *data, struct wl_pointer *p, uint32_t s) { (void)data; (void)p; (void)s; }
static void pointer_axis_stop(void *data, struct wl_pointer *p, uint32_t t, uint32_t a) { (void)data; (void)p; (void)t; (void)a; }
static void pointer_axis_discrete(void *data, struct wl_pointer *p, uint32_t a, int32_t d) { (void)data; (void)p; (void)a; (void)d; }

static const struct wl_pointer_listener pointer_listener = {
    pointer_enter, pointer_leave, pointer_motion, pointer_button, pointer_axis, pointer_frame, pointer_axis_source, pointer_axis_stop, pointer_axis_discrete,
};

static void kb_keymap(void *data, struct wl_keyboard *k, uint32_t format, int32_t fd, uint32_t size) {
    (void)data; (void)k;
    printf("wlhello: keymap format %u, %u bytes\n", format, size);
    fflush(stdout);
    close(fd);
}
static void kb_enter(void *data, struct wl_keyboard *k, uint32_t serial, struct wl_surface *s, struct wl_array *keys) { (void)data; (void)k; (void)serial; (void)s; (void)keys; }
static void kb_leave(void *data, struct wl_keyboard *k, uint32_t serial, struct wl_surface *s) { (void)data; (void)k; (void)serial; (void)s; }
static void kb_key(void *data, struct wl_keyboard *k, uint32_t serial, uint32_t time, uint32_t key, uint32_t state) {
    struct app *app = data;
    (void)k; (void)serial; (void)time;
    if (state == WL_KEYBOARD_KEY_STATE_PRESSED) {
        printf("wlhello: key %u\n", key);
        fflush(stdout);
        if (key == 1) {
            app->running = 0;
        }
    }
}
static void kb_modifiers(void *data, struct wl_keyboard *k, uint32_t serial, uint32_t d, uint32_t l, uint32_t lo, uint32_t g) { (void)data; (void)k; (void)serial; (void)d; (void)l; (void)lo; (void)g; }
static void kb_repeat(void *data, struct wl_keyboard *k, int32_t rate, int32_t delay) { (void)data; (void)k; (void)rate; (void)delay; }

static const struct wl_keyboard_listener keyboard_listener = { kb_keymap, kb_enter, kb_leave, kb_key, kb_modifiers, kb_repeat };

static void seat_caps(void *data, struct wl_seat *seat, uint32_t caps) {
    struct app *app = data;
    if (caps & WL_SEAT_CAPABILITY_POINTER) {
        wl_pointer_add_listener(wl_seat_get_pointer(seat), &pointer_listener, app);
    }
    if (caps & WL_SEAT_CAPABILITY_KEYBOARD) {
        wl_keyboard_add_listener(wl_seat_get_keyboard(seat), &keyboard_listener, app);
    }
}
static void seat_name(void *data, struct wl_seat *seat, const char *name) { (void)data; (void)seat; (void)name; }
static const struct wl_seat_listener seat_listener = { seat_caps, seat_name };

static void global(void *data, struct wl_registry *registry, uint32_t name, const char *interface, uint32_t version) {
    struct app *app = data;
    if (strcmp(interface, wl_compositor_interface.name) == 0) {
        app->compositor = wl_registry_bind(registry, name, &wl_compositor_interface, 4);
    } else if (strcmp(interface, wl_shm_interface.name) == 0) {
        app->shm = wl_registry_bind(registry, name, &wl_shm_interface, 1);
    } else if (strcmp(interface, xdg_wm_base_interface.name) == 0) {
        app->wm_base = wl_registry_bind(registry, name, &xdg_wm_base_interface, version < 2 ? version : 2);
        xdg_wm_base_add_listener(app->wm_base, &wm_listener, app);
    } else if (strcmp(interface, wl_seat_interface.name) == 0) {
        app->seat = wl_registry_bind(registry, name, &wl_seat_interface, version < 5 ? version : 5);
        wl_seat_add_listener(app->seat, &seat_listener, app);
    }
}

static void global_remove(void *data, struct wl_registry *registry, uint32_t name) { (void)data; (void)registry; (void)name; }
static const struct wl_registry_listener registry_listener = { global, global_remove };

int main(void) {
    struct app app = {0};
    app.display = wl_display_connect(NULL);
    if (!app.display) {
        fprintf(stderr, "wlhello: cannot connect to a Wayland compositor (is hxwayland running?)\n");
        return 1;
    }
    struct wl_registry *registry = wl_display_get_registry(app.display);
    wl_registry_add_listener(registry, &registry_listener, &app);
    wl_display_roundtrip(app.display);
    if (!app.compositor || !app.shm || !app.wm_base) {
        fprintf(stderr, "wlhello: compositor is missing required globals\n");
        return 1;
    }
    wl_display_roundtrip(app.display);
    app.surface = wl_compositor_create_surface(app.compositor);
    app.xdg_surface = xdg_wm_base_get_xdg_surface(app.wm_base, app.surface);
    xdg_surface_add_listener(app.xdg_surface, &xdg_surface_listener, &app);
    app.toplevel = xdg_surface_get_toplevel(app.xdg_surface);
    xdg_toplevel_add_listener(app.toplevel, &toplevel_listener, &app);
    xdg_toplevel_set_title(app.toplevel, "Wayland hello");
    xdg_toplevel_set_app_id(app.toplevel, "wlhello");
    wl_surface_commit(app.surface);
    app.running = 1;
    printf("wlhello: connected\n");
    fflush(stdout);
    while (app.running && wl_display_dispatch(app.display) != -1) {
    }
    printf("wlhello: closed after %d frames\n", app.frames);
    wl_display_disconnect(app.display);
    return 0;
}
