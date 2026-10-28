#include <drm.h>
#include <drm_fourcc.h>
#include <drm_mode.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

static int failures;

static void check(int ok, const char *what) {
    printf("%s %s\n", ok ? "PASS" : "FAIL", what);
    if (!ok)
        failures++;
}

static int xioctl(int fd, unsigned long request, void *arg) {
    int r;
    do {
        r = ioctl(fd, request, arg);
    } while (r < 0 && errno == EINTR);
    return r;
}

static void fill(uint32_t *pixels, uint32_t width, uint32_t height, uint32_t pitch, uint32_t color) {
    for (uint32_t y = 0; y < height; y++)
        for (uint32_t x = 0; x < width; x++)
            pixels[y * (pitch / 4) + x] = ((x / 64 + y / 64) & 1) ? color : 0xff202020;
}

int main(void) {
    int fd = open("/dev/dri/card0", O_RDWR | O_CLOEXEC);
    check(fd >= 0, "open /dev/dri/card0");
    if (fd < 0)
        return 1;

    struct stat st;
    check(fstat(fd, &st) == 0 && S_ISCHR(st.st_mode), "the DRM node is a character device");

    char name[32] = {0};
    struct drm_version version = {0};
    version.name = name;
    version.name_len = sizeof(name) - 1;
    check(xioctl(fd, DRM_IOCTL_VERSION, &version) == 0 && strcmp(name, "hamix") == 0, "DRM_IOCTL_VERSION names the hamix driver");

    struct drm_get_cap cap = {.capability = DRM_CAP_DUMB_BUFFER};
    check(xioctl(fd, DRM_IOCTL_GET_CAP, &cap) == 0 && cap.value == 1, "dumb buffers are supported");

    struct drm_set_client_cap atomic = {.capability = DRM_CLIENT_CAP_ATOMIC, .value = 1};
    check(xioctl(fd, DRM_IOCTL_SET_CLIENT_CAP, &atomic) != 0, "atomic modesetting is refused so clients fall back to legacy KMS");

    uint32_t crtcs[4], connectors[4], encoders[4];
    struct drm_mode_card_res res = {0};
    check(xioctl(fd, DRM_IOCTL_MODE_GETRESOURCES, &res) == 0 && res.count_crtcs == 1 && res.count_connectors == 1, "GETRESOURCES counts one CRTC and one connector");
    res.crtc_id_ptr = (uintptr_t)crtcs;
    res.connector_id_ptr = (uintptr_t)connectors;
    res.encoder_id_ptr = (uintptr_t)encoders;
    res.count_fbs = 0;
    xioctl(fd, DRM_IOCTL_MODE_GETRESOURCES, &res);

    struct drm_mode_modeinfo mode;
    uint32_t encoder_id = 0;
    struct drm_mode_get_connector conn = {.connector_id = connectors[0]};
    xioctl(fd, DRM_IOCTL_MODE_GETCONNECTOR, &conn);
    conn.modes_ptr = (uintptr_t)&mode;
    conn.count_modes = 1;
    conn.encoders_ptr = (uintptr_t)&encoder_id;
    conn.count_encoders = 1;
    conn.count_props = 0;
    int got = xioctl(fd, DRM_IOCTL_MODE_GETCONNECTOR, &conn);
    check(got == 0 && conn.connection == 1 && mode.hdisplay > 0 && mode.vdisplay > 0, "the connector is connected and reports a mode");
    printf("drmcheck: mode %s %ux%u@%u\n", mode.name, mode.hdisplay, mode.vdisplay, mode.vrefresh);

    struct drm_mode_get_encoder enc = {.encoder_id = encoder_id};
    check(xioctl(fd, DRM_IOCTL_MODE_GETENCODER, &enc) == 0 && enc.possible_crtcs == 1, "the encoder drives the CRTC");

    struct drm_mode_create_dumb create = {.width = mode.hdisplay, .height = mode.vdisplay, .bpp = 32};
    check(xioctl(fd, DRM_IOCTL_MODE_CREATE_DUMB, &create) == 0 && create.pitch >= create.width * 4 && create.size >= (uint64_t)create.pitch * create.height, "CREATE_DUMB allocates a buffer");
    struct drm_mode_create_dumb back = {.width = mode.hdisplay, .height = mode.vdisplay, .bpp = 32};
    xioctl(fd, DRM_IOCTL_MODE_CREATE_DUMB, &back);

    struct drm_mode_map_dumb map = {.handle = create.handle};
    check(xioctl(fd, DRM_IOCTL_MODE_MAP_DUMB, &map) == 0, "MAP_DUMB gives an mmap offset");
    uint32_t *front_pixels = mmap(NULL, create.size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, map.offset);
    check(front_pixels != MAP_FAILED, "the dumb buffer maps into the process");
    struct drm_mode_map_dumb map_back = {.handle = back.handle};
    xioctl(fd, DRM_IOCTL_MODE_MAP_DUMB, &map_back);
    uint32_t *back_pixels = mmap(NULL, back.size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, map_back.offset);
    if (front_pixels == MAP_FAILED || back_pixels == MAP_FAILED)
        return 1;
    fill(front_pixels, create.width, create.height, create.pitch, 0xff3070d0);
    fill(back_pixels, back.width, back.height, back.pitch, 0xffd07030);

    uint32_t *again = mmap(NULL, create.size, PROT_READ, MAP_SHARED, fd, map.offset);
    check(again != MAP_FAILED && again[0] == front_pixels[0] && again[create.pitch / 4 * 70 + 70] == front_pixels[create.pitch / 4 * 70 + 70], "a second mapping sees the same pixels");

    struct drm_mode_fb_cmd2 fb = {.width = create.width, .height = create.height, .pixel_format = DRM_FORMAT_XRGB8888};
    fb.handles[0] = create.handle;
    fb.pitches[0] = create.pitch;
    check(xioctl(fd, DRM_IOCTL_MODE_ADDFB2, &fb) == 0 && fb.fb_id != 0, "ADDFB2 creates an XRGB8888 framebuffer");
    struct drm_mode_fb_cmd fb_back = {.width = back.width, .height = back.height, .pitch = back.pitch, .bpp = 32, .depth = 24, .handle = back.handle};
    check(xioctl(fd, DRM_IOCTL_MODE_ADDFB, &fb_back) == 0 && fb_back.fb_id != 0, "legacy ADDFB creates a second framebuffer");

    struct drm_mode_crtc crtc = {.crtc_id = crtcs[0], .fb_id = fb.fb_id, .set_connectors_ptr = (uintptr_t)&connectors[0], .count_connectors = 1, .mode = mode, .mode_valid = 1};
    check(xioctl(fd, DRM_IOCTL_MODE_SETCRTC, &crtc) == 0, "SETCRTC shows the framebuffer");

    struct drm_mode_crtc current = {.crtc_id = crtcs[0]};
    check(xioctl(fd, DRM_IOCTL_MODE_GETCRTC, &current) == 0 && current.fb_id == fb.fb_id && current.mode_valid, "GETCRTC reports the active framebuffer");

    struct drm_mode_fb_dirty_cmd dirty = {.fb_id = fb.fb_id};
    check(xioctl(fd, DRM_IOCTL_MODE_DIRTYFB, &dirty) == 0, "DIRTYFB flushes the framebuffer");

    int flips = 0;
    for (int i = 0; i < 6; i++) {
        struct drm_mode_crtc_page_flip flip = {.crtc_id = crtcs[0], .fb_id = (i & 1) ? fb.fb_id : fb_back.fb_id, .flags = DRM_MODE_PAGE_FLIP_EVENT, .user_data = 100 + i};
        if (xioctl(fd, DRM_IOCTL_MODE_PAGE_FLIP, &flip) != 0)
            break;
        struct pollfd p = {.fd = fd, .events = POLLIN};
        if (poll(&p, 1, 1000) != 1)
            break;
        char buf[256];
        ssize_t n = read(fd, buf, sizeof(buf));
        struct drm_event_vblank *ev = (struct drm_event_vblank *)buf;
        if (n < (ssize_t)sizeof(*ev) || ev->base.type != DRM_EVENT_FLIP_COMPLETE || ev->user_data != (uint64_t)(100 + i))
            break;
        flips++;
    }
    check(flips == 6, "PAGE_FLIP delivers flip-complete events through poll and read");
    sleep(1);

    check(xioctl(fd, DRM_IOCTL_MODE_RMFB, &fb_back.fb_id) == 0, "RMFB removes a framebuffer");
    struct drm_mode_destroy_dumb destroy = {.handle = back.handle};
    check(xioctl(fd, DRM_IOCTL_MODE_DESTROY_DUMB, &destroy) == 0, "DESTROY_DUMB frees a buffer");

    munmap(again, create.size);
    munmap(back_pixels, back.size);
    munmap(front_pixels, create.size);
    close(fd);
    printf("drmcheck: %d failure(s)\n", failures);
    return failures ? 1 : 0;
}
