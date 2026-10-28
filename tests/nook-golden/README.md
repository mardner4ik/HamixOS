# Nook golden reference

`cargo xtask golden` boots `out/hamix_os.iso`, logs in as `user`, starts Nook and captures
five scenes at 1024x768 (desktop, dock hover, launcher, a Terminal window, the
Applications menu). Each is compared with the PNG here; more than 1% of changed pixels
(the clock in the top bar is masked) fails. `parameters.md` lists the timing and layout
constants of `userland/desktop/hxserver/src`, and any change to it fails as well.

A deliberate change of Nook's look or motion updates the reference with
`cargo xtask golden --update` in the same change, with the reason recorded below.

## Changes of the reference

* 2026-09-28: first recording.
* 2026-09-28: launcher -- Notes and Videos showed 48 px icons over their rows; the icon
  cache handed out pointers that moved when new icons were inserted. Fixed in
  `launcher.rs`, launcher scene re-recorded.
