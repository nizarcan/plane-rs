// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Decision spike for DECISIONS.md D6: rusty-s3 0.11 on the `get_plain` golden
//! case. Not a workspace member and not used by the server; see README.md.

use rusty_s3::Method;
use url::Url;

fn main() {
    let date: jiff::Timestamp = "2026-11-16T12:34:56Z".parse().unwrap();
    let url = Url::parse("http://localhost:18080/uploads/ws/0f8f-file%20name.png").unwrap();
    let query = [(
        "response-content-disposition",
        "attachment; filename*=UTF-8''file%20name.png",
    )];
    let signed = rusty_s3::signing::sign(
        &date,
        Method::Get,
        url,
        "AKID",
        "SECRET",
        None,
        "us-east-1",
        3600,
        query.iter().copied(),
        std::iter::empty::<(&str, &str)>(),
    );
    println!("{signed}");
}
