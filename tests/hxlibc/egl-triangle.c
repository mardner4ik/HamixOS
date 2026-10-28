#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES2/gl2.h>
#include <stdio.h>
#include <string.h>

static GLuint shader(GLenum type, const char *source)
{
    GLuint s = glCreateShader(type);
    glShaderSource(s, 1, &source, NULL);
    glCompileShader(s);
    GLint ok = 0;
    glGetShaderiv(s, GL_COMPILE_STATUS, &ok);
    if (!ok) {
        char log[512];
        glGetShaderInfoLog(s, sizeof(log), NULL, log);
        printf("FAIL shader: %s\n", log);
    }
    return s;
}

int main(void)
{
    PFNEGLGETPLATFORMDISPLAYEXTPROC get_display = (PFNEGLGETPLATFORMDISPLAYEXTPROC)eglGetProcAddress("eglGetPlatformDisplayEXT");
    EGLDisplay display = get_display ? get_display(EGL_PLATFORM_SURFACELESS_MESA, EGL_DEFAULT_DISPLAY, NULL) : EGL_NO_DISPLAY;
    EGLint major = 0, minor = 0;
    if (display == EGL_NO_DISPLAY || !eglInitialize(display, &major, &minor)) {
        printf("FAIL eglInitialize: 0x%x\n", eglGetError());
        return 1;
    }
    printf("PASS EGL %d.%d (%s)\n", major, minor, eglQueryString(display, EGL_VENDOR));
    eglBindAPI(EGL_OPENGL_ES_API);
    const EGLint config_attrs[] = { EGL_SURFACE_TYPE, EGL_PBUFFER_BIT, EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT,
                                    EGL_RED_SIZE, 8, EGL_GREEN_SIZE, 8, EGL_BLUE_SIZE, 8, EGL_NONE };
    EGLConfig config;
    EGLint count = 0;
    if (!eglChooseConfig(display, config_attrs, &config, 1, &count) || count == 0) {
        printf("FAIL eglChooseConfig: 0x%x\n", eglGetError());
        return 1;
    }
    const EGLint pbuffer_attrs[] = { EGL_WIDTH, 64, EGL_HEIGHT, 64, EGL_NONE };
    EGLSurface surface = eglCreatePbufferSurface(display, config, pbuffer_attrs);
    const EGLint context_attrs[] = { EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE };
    EGLContext context = eglCreateContext(display, config, EGL_NO_CONTEXT, context_attrs);
    if (surface == EGL_NO_SURFACE || context == EGL_NO_CONTEXT || !eglMakeCurrent(display, surface, surface, context)) {
        printf("FAIL context: 0x%x\n", eglGetError());
        return 1;
    }
    printf("PASS GL_RENDERER %s, GL_VERSION %s\n", glGetString(GL_RENDERER), glGetString(GL_VERSION));
    GLuint program = glCreateProgram();
    glAttachShader(program, shader(GL_VERTEX_SHADER, "attribute vec2 p; void main() { gl_Position = vec4(p, 0.0, 1.0); }"));
    glAttachShader(program, shader(GL_FRAGMENT_SHADER, "precision mediump float; void main() { gl_FragColor = vec4(1.0, 0.0, 0.0, 1.0); }"));
    glBindAttribLocation(program, 0, "p");
    glLinkProgram(program);
    glUseProgram(program);
    static const GLfloat triangle[] = { -0.8f, -0.8f, 0.8f, -0.8f, 0.0f, 0.8f };
    glViewport(0, 0, 64, 64);
    glClearColor(0.0f, 0.0f, 1.0f, 1.0f);
    glClear(GL_COLOR_BUFFER_BIT);
    glVertexAttribPointer(0, 2, GL_FLOAT, GL_FALSE, 0, triangle);
    glEnableVertexAttribArray(0);
    glDrawArrays(GL_TRIANGLES, 0, 3);
    glFinish();
    unsigned char center[4] = { 0 }, corner[4] = { 0 };
    glReadPixels(32, 24, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, center);
    glReadPixels(1, 62, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, corner);
    int ok = center[0] == 255 && center[2] == 0 && corner[0] == 0 && corner[2] == 255;
    printf("%s softpipe drew a triangle (center %d,%d,%d corner %d,%d,%d)\n", ok ? "PASS" : "FAIL",
           center[0], center[1], center[2], corner[0], corner[1], corner[2]);
    eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
    eglTerminate(display);
    return ok ? 0 : 1;
}
