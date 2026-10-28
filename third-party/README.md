# Third-party license sources

Diffkin itself is [MIT licensed](../LICENSE). Its dependencies retain their own licenses.

`licenses/manifest.json` supplements dependency packages that omit their upstream license files. Entries are pinned by package name and version. Most sources are retrieved from the exact Git commit in the published crate's `.cargo_vcs_info.json`; source URLs identify those commits. Each local text filename is its SHA-256 digest, which the generator verifies before using it.

Exceptions are recorded in the manifest:

- `libappindicator-sys` and the two `winapi` target packages use the license files supplied by their published parent workspace package, whose Cargo manifest declares the sibling dependency.
- `r-efi` includes its full licensing and copyright statements in `AUTHORS`.
- `selectors` source files refer to the canonical Mozilla Public License 2.0. The supplemental material includes that source notice and the canonical license text supplied by the cached `cssparser` package.

Run `pnpm licenses:generate` to combine these sources with license files from the installed dependency packages. The generated inventory records the application version, lockfile hashes, upstream source archives, declarations, and license texts. Missing license text fails generation. Files are placed in `target/third-party-notices`, bundled with the desktop application, and included in CLI archives.

The inventory includes Cargo build/test dependencies and other platform packages as a conservative superset. It does not inventory operating-system libraries, the system WebView or Git, toolchain components, or installer helper downloads. It does not relicense dependencies or certify their compatibility. Unmodified upstream Cargo source packages can be obtained using the versioned source-archive links in the notices.
