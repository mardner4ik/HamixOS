# Contributing to HamixOS

1. **Everything generated lives in `out/`.** No images, ISOs or root filesystems in the
   repository root.
2. **One entry point:** `cargo xtask` (`build`, `run`, `test`, `lint-layers`, `abi`,
   `hxlibc-build`, `membench`, `kbench`), each with `--arch`. See `docs/guides/BUILDING.md`.
3. **`Cargo.lock` is committed.** `hxlibc/vendor` is pinned to an mlibc release tag; every
   recipe (`ports/<role>/<name>/HXBUILD`) pins its source.
4. **The one-way layer rule:** nothing outside `layers/linux/` may depend on it.
   `cargo xtask lint-layers` and the `hamix-core` boot tests enforce it.
5. **A new native capability is a Hamix ABI call first:** add the row to
   `docs/reference/hamix-abi.md`, run `cargo xtask abi`, then implement it.
6. **Naming:** `hx*` for native system components, `nook-*` for parts of Nook, upstream
   names for upstream software (`xfce4-panel`, not `hx-xfce-panel`).
7. **Before a merge:** `cargo xtask build` for x86_64, aarch64 and riscv64,
   `cargo xtask test` for both profiles, `cargo xtask lint-layers`, `cargo xtask abi --check`.
8. **Decisions are ADRs** in `docs/adr/`, one decision per file; documents carry a status
   (see `docs/README.md`).
