#ifndef HAMIX_HXPROTO_H
#define HAMIX_HXPROTO_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define HXPROTO_VERSION 2

typedef struct hx_window hx_window;

enum hx_event_type {
	HX_EVENT_NONE = 0,
	HX_EVENT_MOUSE = 1,
	HX_EVENT_KEY = 2,
	HX_EVENT_CLOSE = 3,
	HX_EVENT_FOCUS = 4,
	HX_EVENT_RESIZE = 5,
	HX_EVENT_THEME = 6,
	HX_EVENT_SCREEN = 7,
	HX_EVENT_PLACED = 8,
	HX_EVENT_OTHER = 99
};

enum hx_mouse_kind {
	HX_MOUSE_MOVE = 0,
	HX_MOUSE_PRESS = 1,
	HX_MOUSE_RELEASE = 2,
	HX_MOUSE_WHEEL = 3,
	HX_MOUSE_LEAVE = 4
};

enum hx_key {
	HX_KEY_UP = -1,
	HX_KEY_DOWN = -2,
	HX_KEY_LEFT = -3,
	HX_KEY_RIGHT = -4,
	HX_KEY_HOME = -5,
	HX_KEY_END = -6,
	HX_KEY_DELETE = -7,
	HX_KEY_PAGE_UP = -8,
	HX_KEY_PAGE_DOWN = -9,
	HX_KEY_F1 = -41,
	HX_KEY_F12 = -52,
	HX_KEY_INSERT = -53
};

#define HX_MOD_SHIFT 1u
#define HX_MOD_ALT 2u
#define HX_MOD_CTRL 4u

enum hx_cursor {
	HX_CURSOR_ARROW = 0,
	HX_CURSOR_TEXT = 1,
	HX_CURSOR_HAND = 2,
	HX_CURSOR_MOVE = 3,
	HX_CURSOR_RESIZE_NS = 4,
	HX_CURSOR_RESIZE_EW = 5,
	HX_CURSOR_RESIZE_NWSE = 6,
	HX_CURSOR_RESIZE_NESW = 7
};

typedef struct hx_event {
	uint32_t type;
	uint32_t window;
	int32_t x;
	int32_t y;
	uint32_t buttons;
	uint32_t mouse_kind;
	int32_t wheel;
	int32_t key;
	uint32_t mods;
	uint32_t width;
	uint32_t height;
	uint32_t flag;
} hx_event;

const char *hx_version(void);
int hx_available(void);

hx_window *hx_window_create(const char *title, uint32_t width, uint32_t height);
void hx_window_destroy(hx_window *window);
const char *hx_last_error(void);

uint32_t *hx_window_pixels(hx_window *window);
uint32_t hx_window_width(const hx_window *window);
uint32_t hx_window_height(const hx_window *window);
uint32_t hx_window_id(const hx_window *window);
int hx_window_fd(const hx_window *window);

int hx_window_present(hx_window *window);
int hx_window_present_rect(hx_window *window, uint32_t x, uint32_t y, uint32_t width, uint32_t height);
int hx_window_resize(hx_window *window, uint32_t width, uint32_t height);

void hx_window_set_title(hx_window *window, const char *title);
void hx_window_set_app_id(hx_window *window, const char *app_id);
void hx_window_set_icon(hx_window *window, const char *name);
void hx_window_set_cursor(hx_window *window, uint32_t cursor);
void hx_window_set_size_hints(hx_window *window, uint32_t min_width, uint32_t min_height, uint32_t max_width, uint32_t max_height);
void hx_window_set_maximized(hx_window *window, int maximized);
void hx_window_minimize(hx_window *window);
void hx_window_notify(hx_window *window, const char *title, const char *body);
void hx_window_copy_text(hx_window *window, const char *text);

int hx_window_next_event(hx_window *window, hx_event *event, int timeout_ms);

#ifdef __cplusplus
}
#endif

#endif
