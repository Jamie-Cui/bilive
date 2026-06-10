// Copyright (C) 2026 Jamie Cui
// Author: Jamie Cui
// SPDX-License-Identifier: GPL-3.0-or-later

fn main() {
    #[cfg(target_os = "linux")]
    {
        for library in [
            "x11",
            "xrender",
            "xext",
            "xfixes",
            "cairo",
            "pango",
            "pangocairo",
        ] {
            pkg_config::probe_library(library).unwrap_or_else(|error| {
                panic!("failed to find {library} with pkg-config: {error}")
            });
        }
    }

    #[cfg(target_os = "macos")]
    {
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=CoreGraphics");
    }
}
