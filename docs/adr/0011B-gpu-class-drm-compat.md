# ADR-0011B: нативний GPU-клас KPI; `/dev/dri` лише як частина Linux-шару

Status: accepted · Date: 2026-09-27 · Plan: §6.7, трек G

## Контекст
Нативний дисплейний клас KPI (`DisplayOps`: flush, fill, copy, cursor, flip, vblank)
обслуговує hxserver через `SYS_GPU_*`. Mesa очікує DRM-UAPI (`/dev/dri/card0`,
`/dev/dri/renderD128`, `DRM_IOCTL_*`, GEM, dma-buf).

## Рішення
* Нативний клас розширюється можливістю **3D** (`GpuCaps::RENDER`): контексти,
  буфери (resource create/attach/map), подання командних буферів, fence-и. Інтерфейс
  нейтральний до API, форма команд -- як у virgl/Venus (virtio-gpu -- перший провайдер).
* `hx-drm-compat` -- частина Linux-шару: реєструє `/dev/dri/*`, транслює
  `DRM_IOCTL_VERSION/GET_CAP/MODE_*/GEM_*/PRIME_*` і virtgpu-ioctl у нативний клас.
  Без Linux-шару `/dev/dri` не існує.
* Порядок: swrast Mesa (без ядра) -> `kmscube` через dumb-буфери -> virgl у QEMU -> розвідка Intel.

## Наслідки
* Нативний стек не залежить від libdrm/gbm/EGL; Mesa -- гість.
* Опційний GPU-бекенд hxserver (SHOULD) працює напряму через нативний клас.

## Стан реалізації (2026-09-28)
* `hx-drm-compat`, крок 1: `kernel/src/syscall/linux/drm.rs`. Вузол
  `/dev/dri/card0` створюється, коли вмикається Linux-шар, і зникає разом з ним
  (`hxctl linux off`, `linux=0`). Відкрити його може лише процес з ABI Linux.
* Підтримано `VERSION`, `GET_CAP`, `SET_CLIENT_CAP` (atomic відхиляється, тож
  клієнти переходять на legacy KMS), `SET/DROP_MASTER`, `MODE_GETRESOURCES`,
  `GETCONNECTOR` (один віртуальний конектор з поточним режимом),
  `GETENCODER`, `GETCRTC/SETCRTC`, `GETPLANERESOURCES/GETPLANE` (одна primary),
  `CREATE/MAP/DESTROY_DUMB`, `GEM_CLOSE`, `ADDFB`, `ADDFB2`
  (XRGB8888/ARGB8888), `RMFB`, `GETFB`, `DIRTYFB`, `PAGE_FLIP` з подією
  `FLIP_COMPLETE` через `read`/`poll` (темп 60 Гц), `WAIT_VBLANK`.
* Dumb-буфер -- memfd-сегмент; `mmap` на DRM-fd з offset від `MAP_DUMB`
  відображає ті самі сторінки. `SETCRTC` бере екран так само, як `SYS_FB_MAP`
  (зайнятий hxserver-ом екран дає `EACCES`), кадр копіюється в нативний
  framebuffer і скидається через `gpu::mark_dirty` + `flush_pending`; при
  перемиканні VT працює наявний механізм тіньового буфера.
* Перевірка: `tests/boot/linux-drm.hsh` (`drmcheck-musl`, динамічна і
  статична збірки), скріншот QEMU показує обидва буфери під час `PAGE_FLIP`.
* Далі: `renderD128` і PRIME, swrast Mesa + `kmscube`, virgl.
