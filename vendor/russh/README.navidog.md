# russh 0.64.1 compatibility patch

Exact upstream crates.io 0.64.1 source under Apache-2.0. Dependency pins move RSA from rc.18 to rc.19 and PKCS#1 from rc.4 to rc.5 to coexist with gpui-shell 0.8.1. The PKCS#1 parser names RsaPrivateKeyRef explicitly after the generic owned/borrowed API change. No protocol or cryptographic algorithm changes.

Remove this patch when published russh supports the RSA version required by Shell. Validate SSH keys and tunnels, cargo audit, and cargo deny before release.
