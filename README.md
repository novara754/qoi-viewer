# QOI Viewer

A basic app to view QOI images.

Written in Rust using SDL2.

## Building

Requires the [Rust compiler and Cargo](https://www.rust-lang.org/) to be installed.

Requires SDL2 to be installed according to [these instructions](https://github.com/Rust-SDL2/rust-sdl2?tab=readme-ov-file#requirements).

Run the following to compile in the projects root directory:
```
cargo build --release
```

The resulting binary can be found at ./target/release/qoi_viewer

## Running

```
./qoi_viewer <qoi-file>
```

Test images can be found at the official QOI project website: [https://qoiformat.org/](https://qoiformat.org/).
