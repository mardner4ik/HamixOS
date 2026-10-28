#include <cairo.h>
#include <stdio.h>
#include <stdint.h>

int main(void)
{
    int failures = 0;
    cairo_surface_t *surface = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, 64, 64);
    cairo_t *cr = cairo_create(surface);
    cairo_set_source_rgb(cr, 1, 1, 1);
    cairo_paint(cr);
    cairo_set_source_rgb(cr, 0, 0, 1);
    cairo_rectangle(cr, 16, 16, 32, 32);
    cairo_fill(cr);
    cairo_set_source_rgb(cr, 1, 0, 0);
    cairo_set_line_width(cr, 2);
    cairo_arc(cr, 32, 32, 30, 0, 6.2831853);
    cairo_stroke(cr);
    cairo_surface_flush(surface);
    uint32_t *pixels = (uint32_t *)cairo_image_surface_get_data(surface);
    int stride = cairo_image_surface_get_stride(surface) / 4;
    int center_blue = pixels[32 * stride + 32] == 0xff0000ff;
    int corner_white = pixels[0] == 0xffffffff;
    printf("%s cairo fills and strokes\n", center_blue && corner_white ? "PASS" : "FAIL");
    failures += !(center_blue && corner_white);
    cairo_status_t written = cairo_surface_write_to_png(surface, "/tmp/cairo-test.png");
    cairo_surface_t *loaded = cairo_image_surface_create_from_png("/tmp/cairo-test.png");
    int roundtrip = written == CAIRO_STATUS_SUCCESS && cairo_surface_status(loaded) == CAIRO_STATUS_SUCCESS &&
        cairo_image_surface_get_width(loaded) == 64 &&
        ((uint32_t *)cairo_image_surface_get_data(loaded))[32 * (cairo_image_surface_get_stride(loaded) / 4) + 32] == 0xff0000ff;
    printf("%s cairo PNG round trip\n", roundtrip ? "PASS" : "FAIL");
    failures += !roundtrip;
    cairo_surface_destroy(loaded);
    cairo_destroy(cr);
    cairo_surface_destroy(surface);
    printf("cairo %s: %d failure(s)\n", cairo_version_string(), failures);
    return failures ? 1 : 0;
}
