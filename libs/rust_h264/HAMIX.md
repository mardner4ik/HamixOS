# rust_h264 in HamixOS

Upstream: https://github.com/roticv/rust_h264, version 0.4.0 (MIT OR
Apache-2.0, see `LICENSE-MIT` and `LICENSE-APACHE`).

## Port to no_std

* `#![cfg_attr(not(test), no_std)]` with `alloc`; a small prelude module brings
  `Vec`, `vec!` and `ToOwned`/`ToString` into every file.
* `std::sync::OnceLock` is replaced by an atomic `OnceLock` in `src/once.rs`.
* `HashMap` → `BTreeMap`, `std::rc::Rc` → `alloc::rc::Rc`, `std::fmt`/`Error`
  → `core`.

## Bug fixes

All found by decoding streams produced by x264 and comparing every frame with
FFmpeg's output.

1. **Chroma DC dequantisation** (`residual.rs`): a rounding term was added for
   4:2:0 chroma DC, but H.264 8.5.11.2 specifies
   `dcC = ((f * LevelScale) << (qP / 6)) >> 5` without rounding. Visible at low
   QP.
2. **POC type 1 and 2 ignored `FrameNumOffset`** (`dpb.rs`): after `frame_num`
   wrapped around, picture order counts restarted from zero. Baseline streams
   from x264 (POC type 2, `log2_max_frame_num` = 4) broke after 16 frames.
3. **P reference list ordered by POC instead of FrameNumWrap** (`dpb.rs`):
   wrong as soon as B-pictures are used as references (B-pyramids) or
   `frame_num` wraps. MMCO 1/3 picture numbers now wrap modulo MaxFrameNum.
4. **Reference list modification searched the truncated list** (`dpb.rs`):
   when x264 duplicates references for weighted prediction, a modification
   command could refer to a picture that an earlier command had already pushed
   out of the list, and was ignored. Pictures are now looked up in the initial
   list.
5. **CAVLC with 8×8 transform** (`decode_cavlc.rs`, `slice_context.rs`): the
   `transform_size_8x8_flag` was never recorded for deblocking (internal 4×4
   edges were filtered), and the per-4×4 non-zero flags used for boundary
   strength did not cover the whole 8×8 block.
6. **Deblocking used the cropped width as stride** (`deblock.rs`): frames whose
   width is not a multiple of 16 (854×480, 1366×768…) were filtered at the
   wrong positions.
7. **Every slice used the first PPS** (`decoder.rs`): the PPS id from the slice
   header is now used.

## Performance

* `Decoder::decode_nal_shared` / `flush_shared` return a `SharedFrame` that
  shares the planes with the reference buffer instead of cloning them (and a
  cropped copy); `decode_nal` / `flush` still return owned frames.
* Picture planes, motion vector stores and all per-macroblock arrays are
  recycled between pictures instead of being allocated and zeroed each time.
* B-skip/direct macroblocks with the same motion in a 16×16 or 8×8 area are
  predicted as one block instead of sixteen 4×4 blocks.
* `src/x86.rs`: SSE2 6-tap horizontal, vertical and diagonal half-pel filters,
  quarter-pel averaging, chroma bilinear interpolation and bi-prediction
  averaging. Implicit weighted bi-prediction and explicit weighted
  uni-prediction use 16-bit arithmetic where it is exact.
* The state backup made before every continuation slice is skipped for CABAC
  streams, where it is never needed.

Measured on one core of an i5-6300U: 480p high-bitrate content 33 → 77 fps,
720p 53 → ~80 fps, 1080p ~32 fps.
