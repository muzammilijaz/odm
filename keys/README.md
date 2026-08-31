# Extension signing key

This folder holds the Chrome extension's signing key material, used to keep
the extension's ID stable across reloads/rebuilds during development.

- `extension_id.txt` / `public_key_base64.txt` — public, committed. The
  base64 value is also baked into `extension/manifest.json`'s `"key"` field,
  and the ID (`igjebnkcfkjpleeahgjnpdkahplddfdc`) is what
  `extension/native-host-manifest/com.odm.nativehost.json`'s
  `allowed_origins` is pinned to.
- `key.pem` / `key.der` — **private, gitignored**. Anyone with this key could
  sign an extension that Chrome treats as the same ID as the official one, so
  it's never committed.

If you're contributing and don't have `key.pem`, generate your own for local
development:

```sh
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out keys/key.pem
openssl rsa -in keys/key.pem -pubout -outform DER | openssl base64 -A > keys/public_key_base64.txt
```

Then replace `extension/manifest.json`'s `"key"` field with the new base64
value, reload the unpacked extension in `chrome://extensions`, note the new
ID Chrome assigns it, and update
`extension/native-host-manifest/com.odm.nativehost.json`'s
`allowed_origins` and `install-native-host.ps1`'s expectations to match.
