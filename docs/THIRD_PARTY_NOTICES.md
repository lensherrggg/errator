# Third-party notices

Errator is MIT-licensed. The optional Windows release bundles the following
runtime component in addition to Rust crates listed in `Cargo.lock`:

## ONNX Runtime

- Project: https://github.com/microsoft/onnxruntime
- License: MIT
- Version: 1.22.x, distributed through the `ort` / `ort-sys` crates.
- The runtime is linked by the optional `semantic` feature.

## DirectML.dll

- Project: https://github.com/microsoft/DirectML
- DirectML is a Microsoft redistributable component used by the ONNX Runtime
  DirectML execution provider. It is included only in Windows packages built
  with `--features semantic-directml`.
- See the DirectML repository and the ONNX Runtime distribution for the
  applicable Microsoft license terms.

## Microsoft Visual C++ runtime

The portable Windows package places the following app-local runtime libraries
next to `errator.exe` for offline use on machines without the Visual C++
Redistributable installed:

- `vcruntime140.dll`
- `vcruntime140_1.dll`
- `msvcp140.dll`
- `msvcp140_1.dll`

These files are Microsoft Visual C++ Redistributable runtime components; their
redistribution is governed by the Microsoft Visual Studio license terms.
The regular non-portable package relies on the runtime already being present on
the target system.

## Rust dependencies

All Rust crate licenses are recorded in `Cargo.lock` and can be audited with
`cargo metadata` / `cargo deny`. The core binary does not bundle model weights.

Model files downloaded by the user are not part of the Errator distribution and
remain subject to their own licenses.
