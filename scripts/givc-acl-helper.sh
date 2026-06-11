#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
# SPDX-License-Identifier: Apache-2.0

cd ./tools/givc-acl-helper || exit
go run . -d ../../api/
