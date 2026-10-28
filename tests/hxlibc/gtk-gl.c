#include <epoxy/gl.h>
#include <gtk/gtk.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int rendered;
static int correct;
static GLuint program;
static GLuint vao;

static GLuint shader(GLenum type, const char *source)
{
    GLuint s = glCreateShader(type);
    glShaderSource(s, 1, &source, NULL);
    glCompileShader(s);
    return s;
}

static void on_realize(GtkGLArea *area, gpointer data)
{
    gtk_gl_area_make_current(area);
    if (gtk_gl_area_get_error(area)) {
        printf("FAIL GtkGLArea: %s\n", gtk_gl_area_get_error(area)->message);
        return;
    }
    printf("PASS GtkGLArea context: %s, %s\n", glGetString(GL_RENDERER), glGetString(GL_VERSION));
    program = glCreateProgram();
    glAttachShader(program, shader(GL_VERTEX_SHADER, "attribute vec2 p; attribute vec3 c; varying vec3 v; void main() { v = c; gl_Position = vec4(p, 0.0, 1.0); }"));
    glAttachShader(program, shader(GL_FRAGMENT_SHADER, "#ifdef GL_ES\nprecision mediump float;\n#endif\nvarying vec3 v; void main() { gl_FragColor = vec4(v, 1.0); }"));
    glBindAttribLocation(program, 0, "p");
    glBindAttribLocation(program, 1, "c");
    glLinkProgram(program);
    static const GLfloat vertices[] = { -0.7f, -0.7f, 1.0f, 0.2f, 0.2f, 0.7f, -0.7f, 0.2f, 1.0f, 0.2f, 0.0f, 0.7f, 0.2f, 0.4f, 1.0f };
    GLuint vbo;
    glGenVertexArrays(1, &vao);
    glBindVertexArray(vao);
    glGenBuffers(1, &vbo);
    glBindBuffer(GL_ARRAY_BUFFER, vbo);
    glBufferData(GL_ARRAY_BUFFER, sizeof(vertices), vertices, GL_STATIC_DRAW);
    glVertexAttribPointer(0, 2, GL_FLOAT, GL_FALSE, 5 * sizeof(GLfloat), (void *)0);
    glVertexAttribPointer(1, 3, GL_FLOAT, GL_FALSE, 5 * sizeof(GLfloat), (void *)(2 * sizeof(GLfloat)));
    glEnableVertexAttribArray(0);
    glEnableVertexAttribArray(1);
    glBindVertexArray(0);
}

static gboolean on_render(GtkGLArea *area, GdkGLContext *context, gpointer data)
{
    int w = gtk_widget_get_allocated_width(GTK_WIDGET(area)) * gtk_widget_get_scale_factor(GTK_WIDGET(area));
    int h = gtk_widget_get_allocated_height(GTK_WIDGET(area)) * gtk_widget_get_scale_factor(GTK_WIDGET(area));
    glClearColor(0.12f, 0.13f, 0.16f, 1.0f);
    glClear(GL_COLOR_BUFFER_BIT);
    glUseProgram(program);
    glBindVertexArray(vao);
    glDrawArrays(GL_TRIANGLES, 0, 3);
    glBindVertexArray(0);
    if (!rendered) {
        unsigned char center[4] = { 0 }, corner[4] = { 0 };
        glReadPixels(w / 2, h / 2, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, center);
        glReadPixels(2, h - 3, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, corner);
        correct = center[0] > 60 && center[1] > 60 && center[2] > 60 && corner[0] < 60 && corner[2] < 60;
        printf("%s GtkGLArea %dx%d drew a triangle (center %d,%d,%d corner %d,%d,%d)\n", correct ? "PASS" : "FAIL", w, h,
               center[0], center[1], center[2], corner[0], corner[1], corner[2]);
        fflush(stdout);
    }
    rendered++;
    return TRUE;
}

static gboolean on_timeout(gpointer data)
{
    gtk_main_quit();
    return G_SOURCE_REMOVE;
}

int main(int argc, char **argv)
{
    if (!gtk_init_check(&argc, &argv)) {
        printf("FAIL gtk_init_check: no Wayland compositor\n");
        return 1;
    }
    GtkWidget *window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_title(GTK_WINDOW(window), "OpenGL on hxlibc");
    gtk_window_set_default_size(GTK_WINDOW(window), 360, 280);
    GtkWidget *area = gtk_gl_area_new();
    gtk_gl_area_set_use_es(GTK_GL_AREA(area), TRUE);
    g_signal_connect(area, "realize", G_CALLBACK(on_realize), NULL);
    g_signal_connect(area, "render", G_CALLBACK(on_render), NULL);
    gtk_container_add(GTK_CONTAINER(window), area);
    g_signal_connect(window, "destroy", G_CALLBACK(gtk_main_quit), NULL);
    gtk_widget_show_all(window);
    int seconds = getenv("GTK_GL_SECONDS") ? atoi(getenv("GTK_GL_SECONDS")) : 6;
    g_timeout_add_seconds(seconds, on_timeout, NULL);
    gtk_main();
    printf("%s GtkGLArea rendered %d frame(s)\n", rendered ? "PASS" : "FAIL", rendered);
    return rendered && correct ? 0 : 1;
}
