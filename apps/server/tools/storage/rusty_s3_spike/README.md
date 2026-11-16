# rusty-s3 spike (DECISIONS.md D6)

A decision artefact, not part of the build. It exists so the D6 measurement can
be reproduced: the same presigned GET case that boto3 records in
`crates/plane-storage/tests/golden/presign.json` (`get_plain`), signed by
`rusty-s3` 0.11.

```sh
cd apps/server/tools/storage/rusty_s3_spike
CARGO_TARGET_DIR=target cargo run --release
```

Observed output (2026-11-16):

```
http://localhost:18080/uploads/ws/0f8f-file%20name.png?X-Amz-Algorithm=AWS4-HMAC-SHA256&...&X-Amz-SignedHeaders=host&response-content-disposition=attachment%3B%20filename%2A%3DUTF-8%27%27file%2520name.png&X-Amz-Signature=a65b44f36f9a90694147a3e9afe7020fc152a23e755cb23512c2fceb7a13942f
```

boto3's URL for the same inputs:

```
http://localhost:18080/uploads/ws/0f8f-file%20name.png?response-content-disposition=attachment%3B%20filename%2A%3DUTF-8%27%27file%2520name.png&X-Amz-Algorithm=...&...&X-Amz-Signature=a65b44f36f9a90694147a3e9afe7020fc152a23e755cb23512c2fceb7a13942f
```

The signature is identical; the query parameter order is not. rusty-s3 writes
the query string sorted by name by construction, while boto3 writes the
operation parameters in serialization order followed by the `X-Amz-*`
parameters. That is why D6 chose the in-house signer. rusty-s3 also has no
presigned POST policy support, so the POST signer would have to be written
anyway.
