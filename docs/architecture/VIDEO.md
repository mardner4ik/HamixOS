# Video

`/usr/bin/hxvideo` ("Videos" in Nook) plays MP4 files with H.264 video and AAC
sound. Everything it needs lives in three libraries that any HamixOS program
can use; like smoltcp for networking, the heavy lifting is done by existing
open-source Rust code that was vendored and ported to `no_std`.

```
libs/rust_h264/     H.264 decoder (rust_h264 0.4.0, MIT OR Apache-2.0), ported to no_std, fixed and sped up
libs/hx264/         HamixOS H.264 API: MP4 samples in, pictures out, YUV→RGB scaler
libs/shiguredo_mp4/ MP4 box library (shiguredo_mp4 2026.5.0, Apache-2.0), no_std, unchanged
libs/hxmp4/         HamixOS MP4 API: tracks, codecs, sample tables, seeking
libs/hxaac/         AAC-LC decoder adapted from Symphonia (MPL-2.0)
apps/hxvideo/       the player
```

## When a library is written natively, and when it comes from Linux

ffmpeg and mpv are available through the Linux compatibility layer
([docs/LINUXULATOR.md](LINUXULATOR.md)), so "why is there a native H.264
decoder at all?" is a fair question — and it is the same question that will
come up for every audio codec, font shaper and crypto library after this one.
The criterion, so it does not get re-decided each time:

**A native implementation is justified only for what sits in the critical path
of starting or everyday use of the OS itself** — what `hxvideo`, `hxserver`,
`hsh` and `hxwayland` reach for by default, where RAM and start-up time are
what the system is judged on. Everything else — needed rarely, or only by
third-party software — goes through the Linux ABI, with no native twin.

* `hx264`/`hxaac`/`hxmp4` stay native. `hxvideo` is a single process with no
  threads and a decoder that does not copy frames; that is what makes 1080p
  playback on weak hardware possible in a few megabytes, instead of pulling
  mpv/ffmpeg through the linuxulator with the whole musl/glibc library tail
  behind them (tens of megabytes of RSS before a frame is decoded). It is the
  same reasoning that gives the OS its own TCP/IP stack (`libs/smoltcp`)
  instead of a forked Linux network stack.
* The native codec stack does **not** grow past H.264/AAC/MP4 under our own
  steam — no VP9, AV1, Opus or Matroska. For rare formats the RAM difference
  does not pay for months of `no_std` porting; that case is covered by
  `mpv`/`ffmpeg` from pantry. `hxvideo` stays "a fast player for a typical
  MP4/H.264 file", not "a player for everything".
* `libs/mini_png` is the same choice made earlier and correctly: PNG decoding
  is on the boot path (wallpapers, icons, cursors), so it is native and tiny.
  A future "should we write a native JPEG/WebP decoder?" is answered by the
  paragraph above, not from scratch.

## hx264

```rust
let mut decoder = hx264::Decoder::with_avcc(&avcc)?;      // avcC box payload from the MP4
if let Some(picture) = decoder.decode_sample(&sample, pts_ms)? {
    let planes = picture.planes();                         // Y, U, V, strides, visible size
    scaler.draw(&planes, hx264::ColorMatrix::for_size(w, h), window_pixels, stride, x, y, width, height);
}
decoder.reset();                                           // after a seek, keeps SPS/PPS
hx264::is_disposable(&sample, decoder.length_size());      // non-reference picture?
```

* Baseline, Main and High profile, 8-bit 4:2:0, CAVLC and CABAC, B-frames,
  weighted prediction, multiple slices. Not supported: interlaced (field/MBAFF)
  streams, High 10/4:2:2/4:4:4.
* `decode_sample` decodes one access unit and returns its picture at once, so
  the container's presentation timestamp belongs to it; the player reorders
  pictures by timestamp.
* Pictures are shared with the decoder's reference buffer (`Rc`) — no plane is
  copied — and their buffers are recycled for later pictures.
* `Scaler` converts to `0x00RRGGBB` with BT.601 or BT.709 limited-range
  coefficients and scales with bilinear luma / nearest chroma, in 16-bit fixed
  point that the compiler vectorises. 640×360 → 960×540 takes about 3 ms.

### Changes to rust_h264

The upstream decoder was checked frame by frame against FFmpeg on 19 test
streams (x264 baseline to veryslow, B-pyramids, weighted prediction, 16
B-frames, CAVLC with 8×8 transform, 4 slices, odd sizes up to 1080p). Five bugs
were found and fixed; all streams now decode bit-exactly. See
`libs/rust_h264/HAMIX.md` for the full list, including the performance work
(zero-copy pictures, buffer reuse, merged B-skip prediction, SSE2 motion
compensation): 1.5–2.3× faster than upstream on x86-64.

## hxmp4

```rust
let movie = hxmp4::open(&mut source)?;          // anything implementing hxmp4::Source (read_at)
let video = movie.video().unwrap();              // Track: codec, timescale, samples, edit list
let index = video.keyframe_for_ms(90_000);       // seek target
video.read_sample(&mut source, index, &mut buf)?;
let ms = video.sample_ms(index);                 // presentation time with the edit list applied
```

* `moov` may be at the start or at the end of the file.
* Sample tables (`stts`, `ctts`, `stsc`, `stsz`/`stz2`, `stco`/`co64`, `stss`)
  are flattened into one list per track: offset, size, decode and presentation
  time, sync flag.
* The first edit list entry is applied, so AAC priming samples and B-frame
  composition offsets start at 0 ms.
* Fragmented MP4 (`moov` + `mvex`, then `moof`/`mdat` pairs, as saved by
  YouTube/DASH downloaders and browsers) is supported: `tfhd`/`tfdt`/`trun`
  are decoded with `trex` defaults and appended to the same sample list, so
  seeking and A/V sync work the same way as for regular files.

## hxaac

```rust
let mut decoder = hxaac::Decoder::new(&audio_specific_config)?;
decoder.decode(&packet, &mut pcm)?;              // appends interleaved i16, 1024 frames per packet
```

AAC-LC, 8–96 kHz, mono to 7.1 (more than two channels are downmixed to
stereo). ADTS headers are skipped automatically. HE-AAC (SBR/PS) streams decode
their AAC-LC core, which sounds duller but plays. The output matches FFmpeg to
within rounding (mean difference 0.6 of 32768).

## The player

Single process, no threads:

* Audio is decoded ahead into the kernel stream (about 350 ms).
* The clock is the audio stream: samples written minus the stream delay
  reported by the mixer. Without sound the wall clock is used.
* Video pictures are decoded ahead up to the stream's reorder depth (derived
  from the sample table), kept sorted by presentation time and shown when the
  clock reaches them. If several are due, only the newest is drawn.
* When decoding falls behind by more than 80 ms, samples whose NAL units are
  all non-reference are skipped without decoding; the stream stays in sync and
  catches up. Seeking decodes from the previous key frame and skips the same
  way until the target is reached.
* Only the video rectangle (and the controls when visible) is presented to
  Nook each frame.

Controls: play/pause, ±10 s, seek bar (click or drag), volume (master volume,
the same as Nook's), open file, open folder (plays every video in it, one after
another), full window. Keys: Space pause, Left/Right ±5 s, PgUp/PgDn ±60 s,
Home start, Up/Down volume, M mute, F full window, N/P next/previous video,
O open, D open folder, I statistics. Double click toggles the full window.

Measured in QEMU (KVM, one core of an i5-6300U): 360p and 480p play with no
dropped frames, 720p30 with no dropped frames, 1080p30 decodes about 20
pictures per second and stays in sync by skipping non-reference B-pictures.
