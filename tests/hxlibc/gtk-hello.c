#include <gtk/gtk.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int drawn;
static int clicked;

static void on_clicked(GtkButton *button, gpointer data)
{
    clicked = 1;
}

static gboolean on_draw(GtkWidget *widget, cairo_t *cr, gpointer data)
{
    if (!drawn) {
        drawn = 1;
        printf("PASS gtk window drew %dx%d\n", gtk_widget_get_allocated_width(widget), gtk_widget_get_allocated_height(widget));
        fflush(stdout);
        gtk_button_clicked(GTK_BUTTON(data));
    }
    return FALSE;
}

static gboolean on_timeout(gpointer data)
{
    gtk_main_quit();
    return G_SOURCE_REMOVE;
}

int main(int argc, char **argv)
{
    if (argc > 1 && strcmp(argv[1], "--no-display") == 0) {
        unsetenv("WAYLAND_DISPLAY");
        setenv("XDG_RUNTIME_DIR", "/tmp/gtk-none", 1);
        printf("%s gtk_init_check without a compositor\n", gtk_init_check(&argc, &argv) ? "FAIL" : "PASS");
        return 0;
    }
    if (!gtk_init_check(&argc, &argv)) {
        printf("FAIL gtk_init_check: no Wayland compositor\n");
        return 1;
    }
    printf("PASS gtk %u.%u.%u initialised on %s\n", gtk_get_major_version(), gtk_get_minor_version(), gtk_get_micro_version(),
           G_OBJECT_TYPE_NAME(gdk_display_get_default()));
    const char *want = getenv("GTK_HELLO_THEME");
    if (want) {
        gchar *theme = NULL;
        g_object_get(gtk_settings_get_default(), "gtk-theme-name", &theme, NULL);
        GtkCssProvider *named = gtk_css_provider_get_named(want, NULL);
        printf("%s gtk theme %s (wanted %s)\n", theme && strcmp(theme, want) == 0 && named ? "PASS" : "FAIL", theme ? theme : "(none)", want);
        g_free(theme);
    }
    GtkWidget *window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_title(GTK_WINDOW(window), "GTK on hxlibc");
    gtk_window_set_default_size(GTK_WINDOW(window), 320, 160);
    GtkWidget *box = gtk_box_new(GTK_ORIENTATION_VERTICAL, 8);
    GtkWidget *label = gtk_label_new("Hello from GTK 3 on hxlibc");
    GtkWidget *button = gtk_button_new_with_label("Close");
    g_signal_connect(button, "clicked", G_CALLBACK(on_clicked), NULL);
    gtk_box_pack_start(GTK_BOX(box), label, TRUE, TRUE, 0);
    gtk_box_pack_start(GTK_BOX(box), button, FALSE, FALSE, 0);
    gtk_container_add(GTK_CONTAINER(window), box);
    g_signal_connect(window, "draw", G_CALLBACK(on_draw), button);
    g_signal_connect(window, "destroy", G_CALLBACK(gtk_main_quit), NULL);
    gtk_widget_show_all(window);
    int seconds = getenv("GTK_HELLO_SECONDS") ? atoi(getenv("GTK_HELLO_SECONDS")) : 5;
    g_timeout_add_seconds(seconds, on_timeout, NULL);
    gtk_main();
    printf("%s gtk window was drawn by the compositor\n", drawn ? "PASS" : "FAIL");
    printf("%s gtk button click signal\n", clicked ? "PASS" : "FAIL");
    return drawn && clicked ? 0 : 1;
}
