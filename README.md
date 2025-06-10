<!--
    Copyright 2025 TII (SSRC) and the Ghaf contributors
    SPDX-License-Identifier: CC-BY-SA-4.0
-->
<div align="center">
  <img src="./docs/givc.png" alt="Ghaf-GIVC Logo" width="25%" height="25%" />
  <h1>GRPC Inter-vm Communication Framework</h1>
  <p>TII SSRC Secure Technologies</p>
</div>

<div align="center">

[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-darkgreen.svg)](./LICENSES/LICENSE.Apache-2.0) [![License: CC-BY-SA 4.0](https://img.shields.io/badge/License-CC--BY--SA--4.0-orange.svg)](./LICENSES/LICENSE.CC-BY-SA-4.0) [![Style Guide](https://img.shields.io/badge/docs-Style%20Guide-yellow)](https://github.com/tiiuae/ghaf/blob/main/docs/style_guide.md) [![Contributor Covenant](https://img.shields.io/badge/Contributor%20Covenant-2.1-4baaaa.svg)](./CODE_OF_CONDUCT.md)

</div>

This repository implements a gRPC-based control channel for the [Ghaf Framework](https://github.com/tiiuae/ghaf).
The GIVC (Ghaf/gRPC Inter-Vm Communication) framework is a collection of services to to administrate and control virtual machines, their services, and applications.

For more details please refer to the [documentation](https://ghaf.tii.ae/givc/overview/).

## Documentation

The GIVC documentation site is located at [https://ghaf.tii.ae/givc/overview/](https://ghaf.tii.ae/givc/overview/).

To build the automated API documentation locally, use:
```bash
nix build .#docs
```

## Licensing

The Ghaf team uses several licenses to distribute software and documentation:

| License Full Name | SPDX Short Identifier | Description |
| -------- | ----------- | ----------- |
| Apache License 2.0 | [Apache-2.0](https://spdx.org/licenses/Apache-2.0.html) | Ghaf source code. |
| Creative Commons Attribution Share Alike 4.0 International | [CC-BY-SA-4.0](https://spdx.org/licenses/CC-BY-SA-4.0.html) | Ghaf documentation. |