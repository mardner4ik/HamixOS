# ADR-0016B: Mesa на hxlibc -- softpipe, EGL і libglvnd

Status: accepted · Date: 2026-09-29 · Plan: §4A.7 (щабель 3: `mesa` swrast, потім `virgl`) · Спирається на: ADR-0014B

## Контекст
* Після GTK 3 наступний щабель драбини -- Mesa: без неї нативні програми не мають
  OpenGL, `libepoxy` ходить лише по EGL-заголовках, `GtkGLArea` не працює.
* Апаратного 3D-драйвера в нативному шляху немає; `virgl` потребує 3D у модулі
  `virtio-gpu` (ADR-0011B, крок 2).

## Рішення
* **Mesa 24.2.8, лише softpipe** (`-Dgallium-drivers=softpipe`, без LLVM): OpenGL 3.3
  (compatibility), OpenGL ES 3.1, EGL 1.4; платформи `wayland` і `surfaceless`; без GLX,
  GBM, Vulkan, VA/VDPAU. Картинку на екран Mesa віддає через `wl_shm` hxwayland.
* **libglvnd 1.7.0** -- диспетчер: `libEGL`, `libOpenGL` (desktop GL без GLX),
  `libGLESv2`; Mesa -- постачальник (`libEGL_mesa`, `usr/share/glvnd/egl_vendor.d`).
  GTK/libepoxy шукають `libOpenGL.so.0` для desktop GL -- без glvnd GTK падав.
* **libdrm 2.4.123** без драйверів GPU (Mesa вимагає його для EGL/Wayland).
* Заголовки ядра й libdrm ключуються на `__linux__`; hx-cc його не визначає (hxlibc --
  не Linux), тож рецепти libdrm, libglvnd і mesa передають `-D__linux__` у `c_args`
  явно, а не глобально.
* `cargo xtask ports` тепер веде список файлів кожного пакунка
  (`out/hx-ports/ARCH/files/NAME.list`) і прибирає з кореня портів файли, яких
  перезібраний пакунок більше не ставить (без цього лишався старий `libEGL` Mesa).

## Наслідки
* `tests/boot/native-mesa.sh`: EGL surfaceless + GLES2 і desktop GL (фіксований конвеєр
  і шейдери, клієнтські масиви) малюють трикутник у pbuffer; під Nook `GtkGLArea`
  (GTK -> libepoxy -> glvnd -> Mesa -> hxwayland) малює трикутник і читає пікселі назад.
* +~20 МБ на диску (libgallium 15 МБ); у пам'яті -- лише в процесах із GL.
* Далі: `virgl` після 3D у `virtio-gpu`, llvmpipe (потрібен LLVM на hxlibc), `gtk4`.
