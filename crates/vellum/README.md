# vellum

`no_std`, allocation-free 2D graphics primitives library for HamixOS.

`vellum` is the first of the HamixOS graphics libraries. It does not talk to
any hardware itself — it only defines the shared vocabulary (`Color`,
`Point`, `Rect`) and the `Canvas` trait that any pixel-addressable surface
can implement to get `fill`, `fill_rect`, `draw_point` and
`horizontal_gradient` for free.

`drivers/intel-graphics-driver` implements `Canvas` for its `Framebuffer`
type, which is how a hardware video driver plugs into this library today.

## Status

Stage 1 of the planned HamixOS graphics stack: primitives only.

## Roadmap

- [x] Color / Point / Rect primitives
- [x] `Canvas` trait with basic fill/gradient helpers
- [ ] Line and circle rasterization
- [ ] Bitmap fonts and glyph rendering
- [ ] Simple blitting / sprite compositing
- [ ] Software double buffering helper
