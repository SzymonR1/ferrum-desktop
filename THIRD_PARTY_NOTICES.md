# Third-party notices

Ferrum's source license is Apache-2.0; see [`LICENSE`](LICENSE). The resolved dependency graph is checked by `cargo deny` and is pinned by `Cargo.lock`. This notice records additional license material used by the initial native application; a release must ship these files alongside the binary.

- **Bollard 0.21.1**: Apache-2.0.
- **eframe/egui 0.36.2**, Tokio, and other Rust crates: license expressions are recorded in their package metadata and enforced against `deny.toml`.
- **epaint_default_fonts 0.36.2**: source package declares `(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0`. Its bundled fonts also include Hack/Bitstream Vera and emoji-icon-font notices. Copies are in [`licenses/`](licenses/).
- **clipboard-win 5.4.1** and **error-code 3.4.0**: BSL-1.0; see [`licenses/BSL-1.0.txt`](licenses/BSL-1.0.txt).

The repository currently builds source and does not publish a release artifact. Packaging work must verify that the release bundle includes the applicable notices and any additional licenses found by the locked dependency audit.
