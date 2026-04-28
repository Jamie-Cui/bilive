#!/bin/bash
# Copyright (C) 2026 Jamie Cui
# Author: Jamie Cui
# SPDX-License-Identifier: GPL-3.0-or-later

cargo run -p bilive -- serve --listen 127.0.0.1:22333 --web-dir web
