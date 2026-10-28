#include <hamix/hxproto.h>
#include <stdio.h>
#include <string.h>

static void paint(hx_window *w, uint32_t tint)
{
	uint32_t *px = hx_window_pixels(w);
	uint32_t width = hx_window_width(w), height = hx_window_height(w);
	for (uint32_t y = 0; y < height; y++)
		for (uint32_t x = 0; x < width; x++)
			px[y * width + x] = 0xff000000u | ((x * 255 / width) << 16) | ((y * 255 / height) << 8) | tint;
}

int main(void)
{
	printf("libhxproto %s, protocol %d\n", hx_version(), HXPROTO_VERSION);
	if (!hx_available()) {
		printf("FAIL hx_available: hxserver is not running\n");
		return 1;
	}
	hx_window *w = hx_window_create("libhxproto from C", 320, 200);
	if (!w) {
		printf("FAIL hx_window_create: %s\n", hx_last_error());
		return 1;
	}
	if (!hx_window_pixels(w) || hx_window_width(w) != 320 || hx_window_height(w) != 200 || hx_window_fd(w) < 0) {
		printf("FAIL window state: %ux%u fd %d\n", hx_window_width(w), hx_window_height(w), hx_window_fd(w));
		return 1;
	}
	printf("PASS window %u created\n", hx_window_id(w));
	hx_window_set_app_id(w, "org.hamix.HxprotoTest");
	paint(w, 0x80);
	hx_window_present(w);
	if (hx_window_resize(w, 400, 260) != 1 || hx_window_width(w) != 400) {
		printf("FAIL hx_window_resize\n");
		return 1;
	}
	paint(w, 0x20);
	if (hx_window_present_rect(w, 0, 0, 400, 260) != 0) {
		printf("FAIL hx_window_present_rect\n");
		return 1;
	}
	printf("PASS resized to %ux%u and presented\n", hx_window_width(w), hx_window_height(w));
	hx_event ev;
	int events = 0, r;
	while ((r = hx_window_next_event(w, &ev, 300)) > 0)
		events++;
	if (r < 0) {
		printf("FAIL the connection to hxserver broke\n");
		return 1;
	}
	printf("PASS %d events, connection alive\n", events);
	hx_window_destroy(w);
	if (hx_window_create("bad", 0, 0) != NULL || strcmp(hx_last_error(), "invalid argument") != 0) {
		printf("FAIL invalid size accepted\n");
		return 1;
	}
	printf("PASS libhxproto\n");
	return 0;
}
