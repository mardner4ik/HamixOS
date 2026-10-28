#include <pango/pangocairo.h>
#include <stdio.h>
#include <string.h>

int main(void)
{
    int failures = 0;
    PangoFontMap *map = pango_cairo_font_map_get_default();
    PangoFontFamily **families = NULL;
    int count = 0;
    pango_font_map_list_families(map, &families, &count);
    int noto = 0;
    for (int i = 0; i < count; i++)
        if (strcmp(pango_font_family_get_name(families[i]), "Noto Sans") == 0)
            noto = 1;
    g_free(families);
    printf("%s fontconfig lists %d font families (Noto Sans %s)\n", noto ? "PASS" : "FAIL", count, noto ? "found" : "missing");
    failures += !noto;

    cairo_surface_t *surface = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, 320, 48);
    cairo_t *cr = cairo_create(surface);
    cairo_set_source_rgb(cr, 1, 1, 1);
    cairo_paint(cr);
    cairo_set_source_rgb(cr, 0, 0, 0);
    PangoLayout *layout = pango_cairo_create_layout(cr);
    PangoFontDescription *desc = pango_font_description_from_string("Noto Sans 16");
    pango_layout_set_font_description(layout, desc);
    pango_layout_set_text(layout, "HamixOS \xd0\x9f\xd1\x80\xd0\xb8\xd0\xb2\xd1\x96\xd1\x82 \xd7\xa9\xd7\x9c\xd7\x95\xd7\x9d", -1);
    int width = 0, height = 0;
    pango_layout_get_pixel_size(layout, &width, &height);
    pango_cairo_show_layout(cr, layout);
    cairo_surface_flush(surface);
    unsigned char *data = cairo_image_surface_get_data(surface);
    int stride = cairo_image_surface_get_stride(surface);
    int inked = 0;
    for (int y = 0; y < 48; y++)
        for (int x = 0; x < 320; x++)
            if (data[y * stride + x * 4] < 128)
                inked++;
    int ok = width > 100 && height > 10 && inked > 200;
    printf("%s pango lays out and cairo draws mixed Latin, Cyrillic and Hebrew text (%dx%d px, %d dark pixels)\n", ok ? "PASS" : "FAIL", width, height, inked);
    failures += !ok;
    cairo_surface_write_to_png(surface, "/tmp/pango-test.png");
    pango_font_description_free(desc);
    g_object_unref(layout);
    cairo_destroy(cr);
    cairo_surface_destroy(surface);
    printf("pango %s: %d failure(s)\n", pango_version_string(), failures);
    return failures ? 1 : 0;
}
