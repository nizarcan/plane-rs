This directory holds a self-signed certificate for the live HTTPS tests.

`pinned.test.crt` / `pinned.test.key` are a throwaway key pair generated with
OpenSSL (`CN=pinned.test`, SAN `DNS:pinned.test`, valid for ten years) and
committed so the test suite needs no certificate tooling at run time. They are
test fixtures, not secrets: never use them outside `cargo test -p plane-net`.

Regenerate with:

    cd apps/server/crates/plane-net/tests/fixtures
    openssl req -x509 -newkey rsa:2048 -nodes \
      -keyout pinned.test.key -out pinned.test.crt -days 3650 \
      -subj "/CN=pinned.test" -addext "subjectAltName=DNS:pinned.test"
    chmod 644 pinned.test.key

The live test additionally expects `other.test` to *fail* verification against
this certificate; do not add it to the SAN list.
