// Copyright 2016 Mozilla
//
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

extern crate rustc_version;

use rustc_version::{version, Version};
use std::io::{self, Write};
use std::process::exit;

/// `MIN_VERSION` should be changed when there's a new minimum version of rustc required
/// to build the project.
static MIN_VERSION: &str = "1.69.0";

fn main() {
    // Build scripts legitimately panic on critical errors during build-time checks.
    // There's no reasonable recovery path when rustc version is missing or invalid.
    #[expect(clippy::expect_used, reason = "build script fails fast on invalid environment")]
    let ver = version().expect("Failed to get rustc version");
    #[expect(clippy::expect_used, reason = "MIN_VERSION is a static string, should never fail")]
    let min = Version::parse(MIN_VERSION).expect("Failed to parse MIN_VERSION");
    if ver < min {
        #[expect(clippy::expect_used, reason = "if stderr write fails during build, panic is appropriate")]
        writeln!(
            &mut io::stderr(),
            "Mentat requires rustc {MIN_VERSION} or higher, you were using version {ver}."
        )
        .expect("Failed to write to stderr");
        exit(1);
    }
}
