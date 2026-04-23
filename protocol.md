Message format:
- Header (3 bytes)
    - Tag (u8)
    - Payload size (u16)
- Payload (n bytes)

All numbers are little-endian unless otherwise specified.

Client messages:
- Tag 0: request to open a file. Payload is the object key.
- Tag 1: request to close a file. Payload is the object key.

Server messages:
- Tag 0: response to open request. Payload is the errno value (u16, 0 for success).

TODO: add versioning so nothing blows up if an incompatible hook and daemon talk to each other
