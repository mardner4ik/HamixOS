# Sound

HamixOS has one generic sound stack in the kernel. Like the "High Definition
Audio device" driver Windows uses when no vendor driver is installed, it does
not know individual sound chips: it talks to the standard controller
interfaces and discovers the codec layout at run time, so the same code works
on practically every PC since the early 2000s.

```
kernel/src/drivers/audio/
  mod.rs     device trait, mixer, streams, master volume, audiod thread, hxinit unit
  hda.rs     Intel High Definition Audio (every HDA controller: Intel, AMD, NVIDIA, VIA, SiS…)
  ac97.rs    AC'97 (Intel ICH and compatible bus masters, SiS 7012 quirks)
sdk/hamix_std/src/audio.rs   userspace API
apps/hxsound                 command line tool
```

## Drivers

### Intel High Definition Audio (`hda.rs`)

* PCI class 0x04, subclass 0x03. BAR0 is mapped, bus mastering enabled; on
  Intel chipsets the TCSEL register is cleared as Linux does.
* Controller reset through `GCTL`, codec discovery through `STATESTS`.
* Verbs are sent over the CORB/RIRB rings; if a controller does not answer on
  the rings the driver falls back to the immediate command interface.
* Every codec is scanned: audio function group, widgets, pin configuration
  defaults, pin and amplifier capabilities, connection lists (including range
  entries and long form lists).
* Output pins are chosen from the BIOS pin configuration (line out, speaker,
  headphone; pins marked "no connection" are skipped, HDMI/DisplayPort pins
  are ignored). If the BIOS left the configuration empty, any output capable
  pin is used.
* For every pin a path to a DAC is searched through mixers and selectors
  (depth-first, up to 6 hops). Along the path widgets are powered up (D0),
  connection selects are set, output amplifiers are set to 0 dB and unmuted,
  mixer inputs on the path are unmuted.
* External amplifier enable (EAPD) is switched on for pins that support it,
  headphone pins get `HP_EN`.
* When several codecs exist (typically an analog codec plus an HDMI codec) the
  codec with speaker/headphone outputs wins.
* Headphone jacks with presence detection are polled every 500 ms; while
  headphones are plugged in the internal speakers are switched off.
* Playback uses the first output stream descriptor: a 16384-frame cyclic
  buffer described by an 8-entry BDL, 48 kHz (or 44.1 kHz if the DAC cannot do
  48 kHz), 16-bit stereo. The play position comes from `LPIB` (or the DMA
  position buffer when `LPIB` stays at zero).
* No interrupts are needed: the mixer thread reads the position.

### AC'97 (`ac97.rs`)

* PCI class 0x04, subclass 0x01; mixer (NAM) and bus master (NABM) I/O BARs.
* Cold reset, codec ready check, master/headphone/PCM volume to 0 dB, variable
  rate audio switched to 48 kHz when supported.
* PCM out uses all 32 BDL entries as one 16384-frame ring. The last valid index
  is moved forward on every poll so the controller never halts. SiS 7012 has
  swapped `SR`/`PICB` registers and counts `PICB` in bytes; both are handled.

QEMU: `-device intel-hda -device hda-output,audiodev=a0` or
`-device AC97,audiodev=a0` with `-audiodev pa,id=a0` (or `wav,path=out.wav`
to record what the system plays).

## Mixer

The mixer runs in the `audiod` kernel thread and fills the device ring buffer
ahead of the hardware play position:

* Any number of streams (up to 16), each with its own sample rate (4–192 kHz)
  and 1 or 2 channels. Mono is duplicated to both channels.
* Every stream is resampled to the device rate with linear interpolation and
  mixed with 32-bit headroom, then the master volume is applied and the result
  is clipped to 16 bits.
* The ring is kept about 60 ms ahead of the play position. If the hardware
  ever overtakes the mixer (an "xrun", e.g. because a slow emulated device
  consumes audio in large bursts) the target latency grows by 40 ms, up to
  250 ms. Position readings that jump backwards are ignored.
* When no stream has been open for 3 seconds the controller stream is stopped.
* A stream's delay (samples written but not yet heard) is known exactly: the
  mixer remembers the device frame at which the stream's last sample was
  mixed. Players use it for audio/video synchronisation.
* Streams belong to processes and disappear when the process exits. A stream
  can also be closed "after playing", so short sounds do not need to wait.

Master volume is 0–100 with a cubic (perceptual) curve, plus mute. It is kept
in the kernel so it works the same in the text console and in Nook.

## Volume keys

The PS/2 keyboard driver recognises the standard extended scancodes that
laptops send for Fn+F-key volume controls and that multimedia keyboards send
for dedicated keys:

| Key | Scancode | Action |
|-----|----------|--------|
| Mute | `E0 20` | toggle mute |
| Volume down | `E0 2E` | −5 % |
| Volume up | `E0 30` | +5 % (also unmutes) |

USB boot keyboards report the same keys as HID usages `0x7F`, `0x80`, `0x81`,
which are translated to the scancodes above. Holding a key repeats it.
Laptops whose Fn keys only raise ACPI events (no scancode) are not covered.

## System calls

| Number | Name | Arguments | Result |
|--------|------|-----------|--------|
| 9170 | `audio_open` | rate, channels, buffer ms (0 = 1000) | stream id |
| 9171 | `audio_write` | id, samples ptr, byte length, flags (1 = do not block) | frames accepted |
| 9172 | `audio_status` | id, out ptr, 64 | 8 × u64: queued, delay, written, capacity, underruns, device rate, played, xruns |
| 9173 | `audio_control` | id, op (1 pause, 2 flush, 3 stream volume, 4 drain), value | 0 |
| 9174 | `audio_close` | id, play remaining samples first (0/1) | 0 |
| 9175 | `audio_volume` | op (0 get, 1 set, 2 mute, 3 step, 4 toggle mute), value | `changes << 16 \| muted << 8 \| level` |
| 9176 | `audio_info` | buffer, length | text: device, driver, rate, outputs, volume, streams |

`/proc/audio` shows the same information as `audio_info`.

## Userspace API (`hamix_std::audio`)

```rust
use hamix_std::audio::{self, Stream};

let stream = Stream::open(44100, 2)?;      // rate, channels
stream.write_all(&samples)?;               // interleaved i16, blocks while the buffer is full
let delay = stream.status().delay;         // frames still to be heard
stream.set_paused(true);
stream.drain();                            // wait until everything was played

audio::set_volume(60);
audio::toggle_mute();
let info = audio::info();                  // device name, driver, rate, outputs
```

`Stream::write` never blocks and returns how many frames were taken, which is
what a player that also has to show video wants.

## Tools and desktop

* `hxsound info`, `hxsound volume [0-100|+N|-N|mute|unmute|toggle]`,
  `hxsound tone [Hz] [seconds]`, `hxsound play file.wav` (8/16-bit PCM WAV).
  Files opens `.wav` files with it.
* Nook has a speaker button next to the network indicator. Clicking it opens a
  popup with the device, a volume slider and a mute button; scrolling over the
  button changes the volume. Changing the volume with the keyboard shows an
  on-screen indicator above the dock. The level is saved in
  `~/.config/nook.conf` (`audio_volume`, `audio_muted`) and restored when Nook
  starts. `/etc/hamix/audio.conf` (`volume=`, `muted=yes`) sets the level used
  at boot.
* Settings → Sound shows the device, driver, outputs and state, has a volume
  slider, a mute button and a test sound.
