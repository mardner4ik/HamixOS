#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(void)
{
    Display *display = NULL;
    for (int i = 0; i < 40 && !display; i++) {
        display = XOpenDisplay(NULL);
        if (!display)
            usleep(250000);
    }
    if (!display) {
        printf("FAIL XOpenDisplay(:0)\n");
        return 1;
    }
    printf("PASS X server %s %d (%d.%d), screen %dx%d\n", ServerVendor(display), VendorRelease(display),
           ProtocolVersion(display), ProtocolRevision(display),
           DisplayWidth(display, DefaultScreen(display)), DisplayHeight(display, DefaultScreen(display)));
    int screen = DefaultScreen(display);
    Window window = XCreateSimpleWindow(display, RootWindow(display, screen), 100, 100, 320, 180, 0,
                                        BlackPixel(display, screen), WhitePixel(display, screen));
    XStoreName(display, window, "X11 on hxlibc");
    XSelectInput(display, window, ExposureMask | StructureNotifyMask);
    XMapWindow(display, window);
    GC gc = XCreateGC(display, window, 0, NULL);
    int exposed = 0;
    for (int i = 0; i < 100 && !exposed; i++) {
        while (XPending(display)) {
            XEvent event;
            XNextEvent(display, &event);
            if (event.type == Expose)
                exposed = 1;
        }
        if (!exposed)
            usleep(50000);
    }
    const char *text = "Hello from X11 on HamixOS";
    XDrawString(display, window, gc, 20, 90, text, strlen(text));
    XFillRectangle(display, window, gc, 20, 110, 120, 20);
    XSync(display, False);
    XWindowAttributes attrs;
    XGetWindowAttributes(display, window, &attrs);
    printf("%s window exposed and viewable (map_state %d)\n", exposed && attrs.map_state == IsViewable ? "PASS" : "FAIL", attrs.map_state);
    int seconds = getenv("X11_WINDOW_SECONDS") ? atoi(getenv("X11_WINDOW_SECONDS")) : 1;
    sleep(seconds);
    XCloseDisplay(display);
    return exposed ? 0 : 1;
}
