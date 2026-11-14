# russh 0.65.1 compatibility patch

Exact upstream crates.io 0.65.1 source under Apache-2.0. Dependency pins move RSA from rc.18 to rc.19 and PKCS#1 from rc.4 to rc.5 to coexist with gpui-shell 0.8.1. The PKCS#1 parser names RsaPrivateKeyRef explicitly after the generic owned/borrowed API change. No protocol or cryptographic algorithm changes to that release. The vendored agent server is identical to crates.io 0.65.1.

Remove this patch when published russh supports the RSA version required by Shell. Validate SSH keys and tunnels, cargo audit, and cargo deny before release.

Path dependencies may be skipped by advisory tools. Check advisories for the upstream version when upgrading; compare the vendored sources with the published crate, reapply only the pins and borrowed type change, and run the live SSH tests.
