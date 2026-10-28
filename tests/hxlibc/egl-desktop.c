#include <EGL/egl.h>
#include <EGL/eglext.h>
#define GL_GLEXT_PROTOTYPES
#include <GL/gl.h>
#include <GL/glext.h>
#include <stdio.h>

int main(int argc, char **argv)
{
    PFNEGLGETPLATFORMDISPLAYEXTPROC get_display = (PFNEGLGETPLATFORMDISPLAYEXTPROC)eglGetProcAddress("eglGetPlatformDisplayEXT");
    EGLDisplay display = get_display(EGL_PLATFORM_SURFACELESS_MESA, EGL_DEFAULT_DISPLAY, NULL);
    eglInitialize(display, NULL, NULL);
    eglBindAPI(EGL_OPENGL_API);
    const EGLint config_attrs[] = { EGL_SURFACE_TYPE, EGL_PBUFFER_BIT, EGL_RENDERABLE_TYPE, EGL_OPENGL_BIT, EGL_RED_SIZE, 8, EGL_NONE };
    EGLConfig config;
    EGLint count = 0;
    eglChooseConfig(display, config_attrs, &config, 1, &count);
    const EGLint pbuffer_attrs[] = { EGL_WIDTH, 64, EGL_HEIGHT, 64, EGL_NONE };
    EGLSurface surface = eglCreatePbufferSurface(display, config, pbuffer_attrs);
    EGLContext context = eglCreateContext(display, config, EGL_NO_CONTEXT, NULL);
    if (!count || !eglMakeCurrent(display, surface, surface, context)) {
        printf("FAIL desktop GL context: 0x%x\n", eglGetError());
        return 1;
    }
    printf("PASS desktop GL %s\n", glGetString(GL_VERSION));
    static const GLfloat triangle[] = { -0.8f, -0.8f, 0.8f, -0.8f, 0.0f, 0.8f };
    glViewport(0, 0, 64, 64);
    glClearColor(0, 0, 1, 1);
    glClear(GL_COLOR_BUFFER_BIT);
    glColor3f(1, 0, 0);
    glEnableClientState(GL_VERTEX_ARRAY);
    glVertexPointer(2, GL_FLOAT, 0, triangle);
    glDrawArrays(GL_TRIANGLES, 0, 3);
    glFinish();
    unsigned char center[4] = { 0 };
    glReadPixels(32, 24, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, center);
    printf("%s client arrays in a compatibility context (center %d,%d,%d)\n", center[0] == 255 && center[2] == 0 ? "PASS" : "FAIL", center[0], center[1], center[2]);
    GLuint vs = glCreateShader(GL_VERTEX_SHADER), fs = glCreateShader(GL_FRAGMENT_SHADER);
    const char *vsrc = "attribute vec2 p; attribute vec3 c; varying vec3 v; void main() { v = c; gl_Position = vec4(p, 0.0, 1.0); }";
    const char *fsrc = "varying vec3 v; void main() { gl_FragColor = vec4(v, 1.0); }";
    glShaderSource(vs, 1, &vsrc, NULL);
    glShaderSource(fs, 1, &fsrc, NULL);
    glCompileShader(vs);
    glCompileShader(fs);
    GLuint program = glCreateProgram();
    glAttachShader(program, vs);
    glAttachShader(program, fs);
    glBindAttribLocation(program, 0, "p");
    glBindAttribLocation(program, 1, "c");
    glLinkProgram(program);
    glUseProgram(program);
    glDisableClientState(GL_VERTEX_ARRAY);
    static const GLfloat colors[] = { 0, 1, 0, 0, 1, 0, 0, 1, 0 };
    glClear(GL_COLOR_BUFFER_BIT);
    glVertexAttribPointer(0, 2, GL_FLOAT, GL_FALSE, 0, triangle);
    glVertexAttribPointer(1, 3, GL_FLOAT, GL_FALSE, 0, colors);
    glEnableVertexAttribArray(0);
    glEnableVertexAttribArray(1);
    glDrawArrays(GL_TRIANGLES, 0, 3);
    glFinish();
    glReadPixels(32, 24, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, center);
    printf("%s generic client arrays with shaders (center %d,%d,%d)\n", center[1] == 255 && center[0] == 0 ? "PASS" : "FAIL", center[0], center[1], center[2]);
    return 0;
}
