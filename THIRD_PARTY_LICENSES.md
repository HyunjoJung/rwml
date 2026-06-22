# Third-Party Licenses

`rdoc` is distributed under the MIT License (see [`LICENSE`](LICENSE)). It links
the following third-party crates, all under permissive licenses compatible with
MIT.

## Direct dependencies

| Crate | License | Repository |
| --- | --- | --- |
| [`cfb`](https://crates.io/crates/cfb) | MIT | mdsteele/rust-cfb |
| [`encoding_rs`](https://crates.io/crates/encoding_rs) | (Apache-2.0 OR MIT) AND BSD-3-Clause | hsivonen/encoding_rs |
| [`thiserror`](https://crates.io/crates/thiserror) | MIT OR Apache-2.0 | dtolnay/thiserror |

License texts for each crate are available in their respective repositories and
in the `~/.cargo` registry cache after a build. No third-party source is vendored
into this crate.

## Format reference

This crate implements the publicly documented Microsoft Word binary format
([MS-DOC]) and the OLE2 Compound File Binary format ([MS-CFB]). No Microsoft
source code or proprietary material is used; only the open specifications.

[MS-DOC]: https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-doc/
[MS-CFB]: https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-cfb/
